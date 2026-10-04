//! Neutral session events (`bonsai.session/1`): a presentation contract, never native payloads.

use serde::{Deserialize, Serialize};

use crate::wire::{EVENT_BYTES, FIELD_BYTES, truncate};

/// Where an `input` came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The dispatch that started the run.
    Dispatch,
    /// A member or the machine owner.
    User,
}

/// Why an input was not delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    /// The session has ended.
    Closed,
    /// Local policy refused it.
    Refused,
    /// Delivery failed.
    Error,
    /// The queue is full.
    Busy,
}

/// Tool call state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolState {
    /// Still running.
    Running,
    /// Succeeded.
    Ok,
    /// Failed.
    Failed,
    /// Canceled.
    Canceled,
    /// Ended without a reported outcome.
    Ended,
}

/// Tool presentation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    /// Shell command.
    Shell,
    /// File read.
    Read,
    /// File edit.
    Edit,
    /// File write.
    Write,
    /// Search.
    Search,
    /// Web fetch.
    Fetch,
    /// MCP tool.
    Mcp,
    /// Sub-agent.
    Agent,
    /// Plan.
    Plan,
    /// Anything else.
    Other,
}

/// To-do item state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoState {
    /// Not started.
    Pending,
    /// In progress.
    Active,
    /// Finished.
    Done,
}

/// One to-do item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    /// Item text.
    pub text: String,
    /// Item state.
    pub state: TodoState,
}

/// Approval request kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    /// Tool permission.
    Tool,
    /// Plan approval.
    Plan,
    /// Question for the user.
    Question,
}

/// Effect of choosing an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Allow.
    Allow,
    /// Deny.
    Deny,
}

/// One answer option; only one-time or this-session grants are ever listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskOption {
    /// Option ID echoed by `session.answer`.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Allow or deny.
    pub effect: Effect,
}

/// One choice inside a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// Label; answers carry it verbatim.
    pub label: String,
    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One question of a `kind: question` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// Answer key.
    pub key: String,
    /// Short header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// Question text.
    pub question: String,
    /// Choices.
    pub options: Vec<Choice>,
    /// Multiple selection allowed.
    pub multi: bool,
    /// Free text allowed.
    pub other: bool,
}

/// Final outcome of a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Allowed.
    Allow,
    /// Denied.
    Deny,
    /// Withdrawn by the provider, the turn's end or the session's end.
    Withdrawn,
}

/// Turn state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    /// Turn began.
    Started,
    /// Turn finished.
    Completed,
    /// Turn failed.
    Failed,
    /// Turn interrupted.
    Aborted,
}

/// Notice level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Information.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
}

/// Context window usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextUsage {
    /// Used tokens.
    pub used: u64,
    /// Window size.
    pub max: u64,
}

/// The twelve event kinds, tagged by `t`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Body {
    /// User message.
    Input {
        /// Input ID.
        id: String,
        /// Message text.
        text: String,
        /// Sender identity.
        by: String,
        /// Sender login.
        #[serde(skip_serializing_if = "Option::is_none")]
        login: Option<String>,
        /// Dispatch or user.
        origin: Origin,
    },
    /// Input that will not be delivered.
    InputRejected {
        /// Input ID.
        id: String,
        /// Reason code.
        code: RejectCode,
        /// Safe detail.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Assistant text delta, appended per `mid`.
    Text {
        /// Message ID.
        mid: String,
        /// Appended text.
        text: String,
    },
    /// Reasoning delta.
    Reasoning {
        /// Optional message ID.
        #[serde(skip_serializing_if = "Option::is_none")]
        mid: Option<String>,
        /// Appended text.
        text: String,
    },
    /// Tool call insert or update.
    Tool {
        /// Call ID.
        id: String,
        /// Native tool name.
        name: String,
        /// State.
        state: ToolState,
        /// Presentation kind.
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<ToolKind>,
        /// One-line summary.
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Input text, at most 16 KiB.
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<String>,
        /// Output text, at most 16 KiB.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
        /// Whether input or output was cut; absent means unknown.
        #[serde(skip_serializing_if = "Option::is_none")]
        truncated: Option<bool>,
    },
    /// Whole to-do list.
    Todo {
        /// Items.
        items: Vec<TodoItem>,
    },
    /// Approval request.
    Ask {
        /// Request ID.
        id: String,
        /// Kind.
        kind: AskKind,
        /// One-line title.
        title: String,
        /// What is being approved, at most 16 KiB; required for `kind: tool`.
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        /// Whether `detail` was cut.
        #[serde(skip_serializing_if = "Option::is_none")]
        truncated: Option<bool>,
        /// Options.
        options: Vec<AskOption>,
        /// Questions for `kind: question`.
        #[serde(skip_serializing_if = "Option::is_none")]
        questions: Option<Vec<Question>>,
    },
    /// Request outcome.
    AskResolved {
        /// Request ID.
        id: String,
        /// Outcome.
        outcome: Outcome,
        /// Who answered.
        #[serde(skip_serializing_if = "Option::is_none")]
        by: Option<String>,
        /// Their login at the time, for display.
        #[serde(skip_serializing_if = "Option::is_none")]
        login: Option<String>,
    },
    /// Turn boundary.
    Turn {
        /// State.
        state: TurnState,
        /// Reason for failed or aborted turns.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Notice.
    Notice {
        /// Level.
        level: Level,
        /// Text.
        text: String,
    },
    /// Token usage snapshot.
    Usage {
        /// Input tokens.
        #[serde(skip_serializing_if = "Option::is_none")]
        input: Option<u64>,
        /// Output tokens.
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<u64>,
        /// Context window.
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<ContextUsage>,
    },
    /// Session ended.
    Closed {
        /// Reason.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// A numbered event as stored and sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// Sequence number inside the epoch.
    pub seq: u64,
    /// Local Unix time in milliseconds.
    pub at: i64,
    /// Kind and fields.
    #[serde(flatten)]
    pub body: Body,
}

impl Event {
    /// Serialize the event.
    ///
    /// # Errors
    ///
    /// Returns the JSON error when serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// Cut a field to [`FIELD_BYTES`], reporting whether it was cut.
#[must_use]
pub fn field(text: &str) -> (String, bool) {
    let (prefix, cut) = truncate(text, FIELD_BYTES);
    (prefix.to_owned(), cut)
}

/// Text of the event that replaces one that cannot be made to fit.
pub const OVERSIZE_NOTICE: &str = "这里有一个事件超过了大小上限,发不出去,已略过";

/// Shrink an event's largest free-text fields until it serializes within [`EVENT_BYTES`].
///
/// Field limits count raw bytes, but JSON escaping can inflate control characters sixfold,
/// so a tool call with two full 16 KiB fields may still exceed the event limit. Shrunk
/// tool and ask fields are marked `truncated`. When no text is left to shrink, trailing list
/// entries are dropped; an event that still does not fit becomes an error notice, so every
/// returned event fits (protocol §2: an oversize event would be rejected by the Hub).
#[must_use]
pub fn fit(mut event: Event) -> Event {
    loop {
        let size = event.to_json().map_or(usize::MAX, |json| json.len());
        if size <= EVENT_BYTES {
            return event;
        }
        if !shrink_largest(&mut event.body) && !drop_entries(&mut event.body, size) {
            event.body = Body::Notice {
                level: Level::Error,
                text: OVERSIZE_NOTICE.to_owned(),
            };
            return event;
        }
    }
}

fn shrink_largest(body: &mut Body) -> bool {
    let (fields, truncated): (Vec<&mut String>, Option<&mut Option<bool>>) = match body {
        Body::Tool {
            name,
            input,
            output,
            title,
            truncated,
            ..
        } => (
            [input.as_mut(), output.as_mut(), title.as_mut(), Some(name)]
                .into_iter()
                .flatten()
                .collect(),
            Some(truncated),
        ),
        Body::Ask {
            detail,
            title,
            options,
            questions,
            truncated,
            ..
        } => {
            let mut fields: Vec<&mut String> = [detail.as_mut(), Some(title)]
                .into_iter()
                .flatten()
                .collect();
            fields.extend(options.iter_mut().map(|option| &mut option.label));
            for question in questions.iter_mut().flatten() {
                fields.push(&mut question.question);
                fields.extend(question.header.as_mut());
                for choice in &mut question.options {
                    fields.push(&mut choice.label);
                    fields.extend(choice.description.as_mut());
                }
            }
            (fields, Some(truncated))
        }
        Body::Todo { items } => (items.iter_mut().map(|item| &mut item.text).collect(), None),
        Body::Input { text, .. }
        | Body::Text { text, .. }
        | Body::Reasoning { text, .. }
        | Body::Notice { text, .. } => (vec![text], None),
        Body::InputRejected { reason, .. }
        | Body::Turn { reason, .. }
        | Body::Closed { reason } => (reason.as_mut().into_iter().collect(), None),
        Body::AskResolved { login, .. } => (login.as_mut().into_iter().collect(), None),
        Body::Usage { .. } => (Vec::new(), None),
    };
    let Some(largest) = fields.into_iter().max_by_key(|field| field.len()) else {
        return false;
    };
    if largest.is_empty() {
        return false;
    }
    let keep = largest.floor_char_boundary(largest.len() / 2);
    largest.truncate(keep);
    if let Some(flag) = truncated {
        *flag = Some(true);
    }
    true
}

/// Drop trailing entries of a list whose texts are already empty, about as many as the event
/// is over the limit (at least one); `false` when none is left.
fn drop_entries(body: &mut Body, size: usize) -> bool {
    let excess = |len: usize| {
        let over = size.saturating_sub(EVENT_BYTES).saturating_mul(len) / size.max(1);
        len - over.saturating_add(1).min(len)
    };
    match body {
        Body::Todo { items } if !items.is_empty() => {
            items.truncate(excess(items.len()));
            true
        }
        Body::Ask {
            questions: Some(questions),
            truncated,
            ..
        } => {
            let Some(question) = questions
                .iter_mut()
                .rev()
                .find(|question| !question.options.is_empty())
            else {
                return false;
            };
            question.options.truncate(excess(question.options.len()));
            *truncated = Some(true);
            true
        }
        _ => false,
    }
}

/// Text budget for one `text`/`reasoning` event, leaving room for the envelope.
pub const TEXT_CHUNK_BYTES: usize = EVENT_BYTES - 4 * 1024;

/// Split a long text into chunks that each fit one event once JSON-escaped.
///
/// Sizes count `serde_json`'s escaping exactly, so every chunk serializes within
/// [`TEXT_CHUNK_BYTES`]. Empty text yields one empty chunk.
#[must_use]
pub fn chunks(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (index, character) in text.char_indices() {
        let cost = escaped_len(character);
        if index > start && size + cost > TEXT_CHUNK_BYTES {
            parts.push(&text[start..index]);
            start = index;
            size = 0;
        }
        size += cost;
    }
    if start < text.len() || parts.is_empty() {
        parts.push(&text[start..]);
    }
    parts
}

fn escaped_len(character: char) -> usize {
    match character {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{8}' | '\u{c}' => 2,
        control if u32::from(control) < 0x20 => 6,
        other => other.len_utf8(),
    }
}

#[cfg(test)]
mod tests;
