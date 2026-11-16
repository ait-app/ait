//! Reading a Bot's records by id in bounded pages (ADR-280): the request the scoped MCP's
//! `bot_read` and `ticket_read` send to [`crate::service::Bots::read`], who may read what, and
//! the inline-and-spill rule every page follows.
//!
//! Lengths count characters (Unicode scalar values), so a page or a preview never splits one.
//! A text field cut to a preview becomes `{"preview","chars","read"}`, where `read` names the
//! resource that returns it whole; a text page is `{"resource","text","offset","chars",
//! "nextOffset"}` and a list page `{"resource","items","nextOffset"}`.

use std::collections::BTreeMap;

use domain::bot::{BOT_CONVERSATION_LABEL, BOT_TICKET_LABEL};
use domain::{ConversationId, TicketId};
use model::ErrorCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Default length of a text page, in characters.
pub(crate) const TEXT_PAGE: usize = 4000;
/// Longest text page, in characters.
pub(crate) const TEXT_PAGE_LIMIT: usize = 8000;
/// Default number of items of a list page.
pub(crate) const LIST_PAGE: usize = 20;
/// Most items of a list page.
pub(crate) const LIST_PAGE_LIMIT: usize = 50;
/// Preview length of each text field of a list item, in characters.
pub(crate) const PREVIEW: usize = 400;
/// Preview length of each text field of a single object's page, in characters.
pub(crate) const DETAIL: usize = 1200;
/// Preview length of each notification summary in a wake input, in characters.
pub(crate) const WAKE_PREVIEW: usize = 600;

/// One read of a Bot resource on behalf of a scoped MCP caller.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// Root Workspace of the caller's Bot.
    pub root: String,
    /// Whose credential asks; decides what may be read.
    pub reader: Reader,
    /// Resource path, such as `ticket/<id>` or, for a worker, `ticket/reports`.
    pub resource: String,
    /// First item or character to return; 0 when absent.
    pub offset: Option<usize>,
    /// Items or characters to return; 20 items or 4000 characters when absent.
    pub limit: Option<usize>,
    /// For `agent/<id>`: the Agent's record from `agent.get.request`, which the caller reads
    /// because the Agent directory is not the Bot's.
    pub agent: Option<Value>,
}

/// The scoped credential a read comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reader {
    /// A Bot session of this conversation.
    Conversation(ConversationId),
    /// An Agent working on this ticket, from its daemon-written `ait.bot.ticket` label.
    Worker(TicketId),
}

/// Where a scoped MCP caller's work came from, which decides what it reaches (ADR-280 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A Bot session of this conversation.
    Conversation(ConversationId),
    /// An Agent of a Bot's subtree, by its daemon-written labels: the ticket it works on
    /// (`ait.bot.ticket`) and the conversation that started it (`ait.bot.conversation`).
    Agent {
        /// The ticket the Agent works on, if any.
        ticket: Option<TicketId>,
        /// The conversation that started the Agent or its ancestor, if any.
        conversation: Option<ConversationId>,
    },
}

/// What a reader may see, resolved against the Bot's channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Viewer {
    /// The built-in Ait conversation of the Workspace owner: every conversation of its Bot.
    Owner(ConversationId),
    /// Any other conversation, including one the Bot no longer has: only itself and what it
    /// originated.
    External(ConversationId),
    /// A worker: only its own ticket.
    Worker(TicketId),
}

impl Viewer {
    /// Whether this viewer may read what conversation `origin` originated (`None`: no
    /// conversation, such as Workspace events and tickets of a closed conversation).
    #[must_use]
    pub fn sees(self, origin: Option<ConversationId>) -> bool {
        match self {
            Self::Owner(_) => true,
            Self::External(own) => origin == Some(own),
            Self::Worker(_) => false,
        }
    }

    /// The reading conversation, for marking what it originated.
    #[must_use]
    pub const fn conversation(self) -> Option<ConversationId> {
        match self {
            Self::Owner(conversation) | Self::External(conversation) => Some(conversation),
            Self::Worker(_) => None,
        }
    }

    /// Whether this viewer may see, direct or stop the Agent with `labels` (ADR-280 §4).
    ///
    /// The owner reaches every Agent and a worker none through this rule. An external
    /// conversation reaches an Agent whose `ait.bot.ticket` is a ticket it originated; when the
    /// Agent has no such label, or the ticket is unknown (`ticket_origin` returns `None`) or kept
    /// without a conversation, it reaches the Agent whose `ait.bot.conversation` is itself. A
    /// ticket of another conversation always refuses, whatever the Agent's own label says,
    /// because the Agent reads that ticket. Agents the Bot did not start carry neither label.
    #[must_use]
    pub fn sees_agent(
        self,
        labels: &Value,
        ticket_origin: impl FnOnce(TicketId) -> Option<Option<ConversationId>>,
    ) -> bool {
        let own = match self {
            Self::Owner(_) => return true,
            Self::Worker(_) => return false,
            Self::External(own) => own,
        };
        let ticket = labels[BOT_TICKET_LABEL]
            .as_str()
            .and_then(|ticket| ticket.parse().ok())
            .and_then(ticket_origin);
        match ticket {
            Some(Some(origin)) => origin == own,
            Some(None) | None => {
                labels[BOT_CONVERSATION_LABEL]
                    .as_str()
                    .and_then(|conversation| conversation.parse().ok())
                    == Some(own)
            }
        }
    }
}

/// A viewer with what it needs to decide on Agents without asking the Bot again: for an
/// external viewer the origin conversation of each ticket of the Bot, empty otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sight {
    /// Who reads.
    pub viewer: Viewer,
    /// Origin conversation of each ticket of the Bot, filled for an external viewer only.
    pub tickets: BTreeMap<TicketId, Option<ConversationId>>,
}

impl Sight {
    /// Whether the viewer may see, direct or stop the Agent of directory record `agent`, by
    /// its labels (see [`Viewer::sees_agent`]).
    #[must_use]
    pub fn sees_agent(&self, agent: &Value) -> bool {
        self.viewer.sees_agent(&agent["labels"], |ticket| {
            self.tickets.get(&ticket).copied()
        })
    }
}

/// `text` as is when it has at most `limit` characters, otherwise its first `limit`
/// characters with its full length and the resource `read()` names, which returns it whole.
#[must_use]
pub fn spill(text: &str, limit: usize, read: impl FnOnce() -> String) -> Value {
    cut(text, limit).map_or_else(
        || json!(text),
        |(preview, chars)| json!({"preview":preview,"chars":chars,"read":read()}),
    )
}

/// The first `limit` characters of `text` and its length in characters, or `None` when it is
/// not longer than `limit`.
fn cut(text: &str, limit: usize) -> Option<(&str, usize)> {
    let (end, _) = text.char_indices().nth(limit)?;
    Some((&text[..end], limit + text[end..].chars().count()))
}

/// The byte index of character `index` of `text`, or its length past the end.
fn byte_index(text: &str, index: usize) -> usize {
    text.char_indices()
        .nth(index)
        .map_or(text.len(), |(byte, _)| byte)
}

/// Offset and length of one page, validated against the page kind's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Page {
    pub(crate) offset: usize,
    pub(crate) limit: usize,
}

impl Page {
    /// A list page of `read`: [`LIST_PAGE`] items by default, at most [`LIST_PAGE_LIMIT`].
    /// # Errors
    /// Returns `InvalidMessage` for a limit of 0 or above the bound.
    pub(crate) fn list(read: &Read) -> Result<Self, ErrorCode> {
        Self::bounded(read, LIST_PAGE, LIST_PAGE_LIMIT)
    }

    /// A text page of `read`: [`TEXT_PAGE`] characters by default, at most
    /// [`TEXT_PAGE_LIMIT`].
    /// # Errors
    /// Returns `InvalidMessage` for a limit of 0 or above the bound.
    pub(crate) fn text(read: &Read) -> Result<Self, ErrorCode> {
        Self::bounded(read, TEXT_PAGE, TEXT_PAGE_LIMIT)
    }

    fn bounded(read: &Read, default: usize, bound: usize) -> Result<Self, ErrorCode> {
        let limit = read.limit.unwrap_or(default);
        if limit == 0 || limit > bound {
            return Err(ErrorCode::InvalidMessage);
        }
        Ok(Self {
            offset: read.offset.unwrap_or(0),
            limit,
        })
    }

    /// This page of `items` as `{"items","nextOffset"}`, projecting only the kept items.
    pub(crate) fn items<T>(self, items: &[T], project: impl FnMut(&T) -> Value) -> Value {
        let start = self.offset.min(items.len());
        let end = start.saturating_add(self.limit).min(items.len());
        let next = (end < items.len()).then_some(end);
        let kept: Vec<Value> = items[start..end].iter().map(project).collect();
        json!({"items":kept,"nextOffset":next})
    }

    /// This page of `text`, read as `resource`.
    pub(crate) fn of_text(self, resource: &str, text: &str) -> Value {
        let chars = text.chars().count();
        let start = byte_index(text, self.offset);
        let end = start + byte_index(&text[start..], self.limit);
        let read = self.offset.saturating_add(self.limit);
        json!({"resource":resource,"text":&text[start..end],"offset":self.offset,"chars":chars,
            "nextOffset":(read < chars).then_some(read)})
    }
}

#[cfg(test)]
mod tests;
