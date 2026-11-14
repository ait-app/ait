//! Resolve `bot_read` and `ticket_read` resources against the Bot's records (ADR-280).
//!
//! Reads only consult the in-memory indexes and the ticket and turn files: they never write,
//! wake, observe or move a conversation's unseen cursor.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use domain::bot::notification::{LoggedNotification, Notification, WakeStatus};
use domain::bot::ticket::{Report, Ticket};
use domain::bot::{
    BOT_CONVERSATION_LABEL, BOT_TICKET_LABEL, Bot, ChannelConfig, Conversation, Turn,
};
use domain::{BotTurnId, ConversationId, TicketId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::State;
use crate::recall::{DETAIL, Origin, PREVIEW, Page, Read, Reader, Sight, Viewer, spill};

/// The MCP tool that reads an Agent's own transcript and result, which no Bot resource repeats.
const AGENT_TRANSCRIPT_TOOL: &str = "get_agent_activity";

/// A parsed resource path; worker paths carry the worker's own ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resource<'a> {
    Tickets { closed: bool },
    Ticket(TicketId),
    TicketReports(TicketId),
    TicketReport(TicketId, u64),
    TicketRequest(TicketId),
    TicketOutcome(TicketId),
    Conversations,
    Conversation(ConversationId),
    Turn(BotTurnId),
    TurnInput(BotTurnId),
    TurnMessage(BotTurnId, usize),
    Notifications,
    Notification(u64),
    Agent(&'a str),
}

impl<'a> Resource<'a> {
    /// Parse `path` for `viewer`: a worker names only its own ticket, without its id.
    fn parse(path: &'a str, viewer: Viewer) -> Option<Self> {
        let mut parts = path.split('/');
        let head = parts.next()?;
        let resource = match viewer {
            Viewer::Worker(own) if head == "ticket" => ticket_part(own, &mut parts)?,
            Viewer::Worker(_) => return None,
            Viewer::Owner(_) | Viewer::External(_) => match head {
                "tickets" => match parts.next() {
                    None => Self::Tickets { closed: false },
                    Some("closed") => Self::Tickets { closed: true },
                    Some(_) => return None,
                },
                "ticket" => ticket_part(parts.next()?.parse().ok()?, &mut parts)?,
                "conversations" => Self::Conversations,
                "conversation" => Self::Conversation(parts.next()?.parse().ok()?),
                "turn" => {
                    let id = parts.next()?.parse().ok()?;
                    match (parts.next(), parts.next()) {
                        (None, _) => Self::Turn(id),
                        (Some("input"), None) => Self::TurnInput(id),
                        (Some("message"), Some(index)) => {
                            Self::TurnMessage(id, index.parse().ok()?)
                        }
                        _ => return None,
                    }
                }
                "notifications" => Self::Notifications,
                "notification" => Self::Notification(parts.next()?.parse().ok()?),
                "agent" => Self::Agent(path.strip_prefix("agent/").filter(|id| !id.is_empty())?),
                _ => return None,
            },
        };
        // Agent ids may contain `/`; every other path must be fully consumed.
        if !matches!(resource, Self::Agent(_)) && parts.next().is_some() {
            return None;
        }
        Some(resource)
    }
}

/// The ticket sub-resource of ticket `id` that the rest of the path names.
fn ticket_part<'a>(
    id: TicketId,
    parts: &mut impl Iterator<Item = &'a str>,
) -> Option<Resource<'a>> {
    Some(match parts.next() {
        None => Resource::Ticket(id),
        Some("reports") => Resource::TicketReports(id),
        Some("report") => Resource::TicketReport(id, parts.next()?.parse().ok()?),
        Some("request") => Resource::TicketRequest(id),
        Some("outcome") => Resource::TicketOutcome(id),
        Some(_) => return None,
    })
}

/// Read pointers as `viewer` writes them: a worker's ticket paths omit the ticket id.
#[derive(Debug, Clone, Copy)]
struct Paths(Viewer);

impl Paths {
    fn ticket(self, id: TicketId, rest: &str) -> String {
        match self.0 {
            Viewer::Worker(_) => format!("ticket{rest}"),
            Viewer::Owner(_) | Viewer::External(_) => format!("ticket/{id}{rest}"),
        }
    }
}

impl State {
    /// The sight of a caller of `root`'s current Bot whose work came from `origin`: that of
    /// the conversation the work came from (a conversation of the Bot's built-in channel is its
    /// owner, any other conversation is external), with, for an external conversation, the
    /// origin conversation of every ticket of the Bot, so a caller can decide on Agents by their
    /// labels.
    ///
    /// An Agent's work comes from the conversation of its ticket when the Bot keeps that ticket
    /// with one, otherwise from its conversation label (ADR-280 §4).
    /// # Returns
    /// `None` for an Agent whose work came from no conversation, which is not restricted.
    /// # Errors
    /// Returns `BotNotFound` when `root` has no current Bot.
    pub(crate) fn sight(&self, root: &str, origin: Origin) -> Result<Option<Sight>, ErrorCode> {
        let bot = self.current(root)?;
        let conversation = match origin {
            Origin::Conversation(conversation) => Some(conversation),
            Origin::Agent {
                ticket,
                conversation,
            } => ticket
                .and_then(|ticket| self.indexed_ticket(bot.id, ticket).ok())
                .and_then(|ticket| ticket.conversation_id)
                .or(conversation),
        };
        let Some(conversation) = conversation else {
            return Ok(None);
        };
        let viewer = self.resolve_viewer(bot, Reader::Conversation(conversation));
        let tickets = match viewer {
            Viewer::External(_) => self
                .tickets
                .values()
                .filter(|ticket| ticket.bot_id == bot.id)
                .map(|ticket| (ticket.id, ticket.conversation_id))
                .collect(),
            Viewer::Owner(_) | Viewer::Worker(_) => BTreeMap::new(),
        };
        Ok(Some(Sight { viewer, tickets }))
    }

    fn resolve_viewer(&self, bot: &Bot, reader: Reader) -> Viewer {
        match reader {
            Reader::Worker(ticket) => Viewer::Worker(ticket),
            Reader::Conversation(id) => {
                let builtin = self.conversation(bot.id, id).is_ok_and(|conversation| {
                    self.channel(bot.id, conversation.channel_id)
                        .is_ok_and(|channel| matches!(channel.config, ChannelConfig::Builtin))
                });
                if builtin {
                    Viewer::Owner(id)
                } else {
                    Viewer::External(id)
                }
            }
        }
    }

    /// Read one resource of `read.root`'s current Bot for `read.reader`.
    /// # Errors
    /// Returns `InvalidMessage` for an unknown resource or page bounds, `BotNotFound` for a
    /// missing Bot or record, `Forbidden` for a record the reader may not read, and `BotIo`
    /// when a stored record cannot be read.
    pub(crate) fn read(&self, read: &Read) -> Result<Value, ErrorCode> {
        let bot = self.current(&read.root)?;
        let viewer = self.resolve_viewer(bot, read.reader);
        let resource = Resource::parse(&read.resource, viewer).ok_or(ErrorCode::InvalidMessage)?;
        let reading = Reading {
            state: self,
            bot,
            viewer,
            paths: Paths(viewer),
            read,
        };
        let mut page = match resource {
            Resource::Tickets { closed } => reading.tickets(closed)?,
            Resource::Ticket(id) => reading.ticket(id)?,
            Resource::TicketReports(id) => {
                let ticket = reading.full_ticket(id)?;
                let mut page = reading.reports(&ticket, Page::list(read)?);
                without(&mut page, "read");
                page
            }
            Resource::TicketReport(id, sequence) => reading.report(id, sequence)?,
            Resource::TicketRequest(id) => {
                let ticket = reading.full_ticket(id)?;
                Page::text(read)?.of_text(&read.resource, &ticket.request)
            }
            Resource::TicketOutcome(id) => {
                let ticket = reading.full_ticket(id)?;
                let outcome = ticket.outcome.as_deref().unwrap_or_default();
                Page::text(read)?.of_text(&read.resource, outcome)
            }
            Resource::Conversations => reading.conversations()?,
            Resource::Conversation(id) => reading.conversation(id)?,
            Resource::Turn(id) => reading.turn(id)?,
            Resource::TurnInput(id) => {
                let turn = reading.readable_turn(id)?;
                Page::text(read)?.of_text(&read.resource, &turn.text)
            }
            Resource::TurnMessage(id, index) => {
                let turn = reading.readable_turn(id)?;
                let replies = replies(&turn);
                let (_, text, _) = replies.get(index).ok_or(ErrorCode::BotNotFound)?;
                Page::text(read)?.of_text(&read.resource, text)
            }
            Resource::Notifications => reading.notifications()?,
            Resource::Notification(sequence) => reading.notification(sequence)?,
            Resource::Agent(id) => reading.agent(id)?,
        };
        if let Some(page) = page.as_object_mut() {
            page.entry("resource")
                .or_insert_with(|| json!(read.resource));
        }
        Ok(page)
    }
}

/// One read in progress: the Bot, the resolved viewer and the request.
struct Reading<'a> {
    state: &'a State,
    bot: &'a Bot,
    viewer: Viewer,
    paths: Paths,
    read: &'a Read,
}

impl Reading<'_> {
    /// `tickets` and `tickets/closed`: open tickets, most recently changed first, or closed
    /// ones, most recently closed first.
    fn tickets(&self, closed: bool) -> Result<Value, ErrorCode> {
        let mut tickets: Vec<&Ticket> = self
            .state
            .tickets
            .values()
            .filter(|ticket| ticket.bot_id == self.bot.id && ticket.status.is_closed() == closed)
            .collect();
        if closed {
            tickets.sort_unstable_by_key(|ticket| Reverse(ticket.closed_at));
        } else {
            tickets.sort_unstable_by_key(|ticket| Reverse(ticket.updated_at));
        }
        Ok(Page::list(self.read)?.items(&tickets, |ticket| self.ticket_item(ticket)))
    }

    /// A ticket as a list item: a card with its newest report, or only its title and status
    /// for a viewer that may not read it.
    fn ticket_item(&self, ticket: &Ticket) -> Value {
        if !self.viewer.sees(ticket.conversation_id) {
            return self.ticket_stub(ticket);
        }
        let mut item = self.ticket_card(ticket);
        item["closedAt"] = json!(ticket.closed_at);
        item["agents"] = json!(ticket.assignments.len());
        item["latestReport"] = ticket.reports.last().map_or(Value::Null, |report| {
            self.report_item(ticket, report, PREVIEW)
        });
        item
    }

    /// Title and status of a ticket the viewer may not read, as the overview shows them; why it
    /// waits, but not which Agent made it wait.
    fn ticket_stub(&self, ticket: &Ticket) -> Value {
        let waiting = ticket
            .waiting
            .as_ref()
            .map(|waiting| json!({"reason":waiting.reason,"since":waiting.since}));
        json!({"id":ticket.id,"title":ticket.title,"status":ticket.status,
            "waiting":waiting,"updatedAt":ticket.updated_at,"closedAt":ticket.closed_at,
            "restricted":true,"read":self.paths.ticket(ticket.id, "")})
    }

    /// The fields every readable ticket view shares, with the conversation it came from when
    /// the viewer may read that conversation.
    fn ticket_card(&self, ticket: &Ticket) -> Value {
        let mut card = json!({"id":ticket.id,"title":ticket.title,"status":ticket.status,
            "waiting":ticket.waiting,"updatedAt":ticket.updated_at,
            "read":self.paths.ticket(ticket.id, "")});
        if let Some(reader) = self.viewer.conversation() {
            card["thisConversation"] = json!(ticket.conversation_id == Some(reader));
            card["conversation"] = json!(
                ticket
                    .conversation_id
                    .map(|conversation| format!("conversation/{conversation}"))
            );
        }
        card
    }

    fn report_item(&self, ticket: &Ticket, report: &Report, limit: usize) -> Value {
        let text = spill(&report.text, limit, || {
            self.paths
                .ticket(ticket.id, &format!("/report/{}", report.sequence))
        });
        json!({"sequence":report.sequence,"at":report.at,"source":report.source,
            "kind":report.kind,"agentId":report.agent_id,"status":report.status,"text":text})
    }

    /// `ticket/<id>`: the whole ticket for a viewer that may read it, otherwise its stub.
    fn ticket(&self, id: TicketId) -> Result<Value, ErrorCode> {
        let indexed = self.state.indexed_ticket(self.bot.id, id)?;
        if !self.may_read(indexed) {
            return Ok(self.ticket_stub(indexed));
        }
        let ticket = self.state.load_ticket(self.bot.id, id)?;
        let mut page = self.ticket_card(&ticket);
        let (paths, id) = (self.paths, ticket.id);
        page["createdAt"] = json!(ticket.created_at);
        page["closedAt"] = json!(ticket.closed_at);
        page["request"] = spill(&ticket.request, DETAIL, || paths.ticket(id, "/request"));
        page["outcome"] = ticket.outcome.as_deref().map_or(Value::Null, |outcome| {
            spill(outcome, DETAIL, || paths.ticket(id, "/outcome"))
        });
        page["assignments"] = ticket
            .assignments
            .iter()
            .map(|assignment| {
                let latest = ticket
                    .reports
                    .iter()
                    .rev()
                    .find(|report| report.agent_id.as_deref() == Some(&assignment.agent_id));
                let mut item = json!({"agentId":assignment.agent_id,
                    "workspaceId":assignment.workspace_id,"assignedAt":assignment.assigned_at,
                    "latestReport":latest.map(|report| self.report_item(&ticket, report, PREVIEW))});
                if !matches!(self.viewer, Viewer::Worker(_)) {
                    item["read"] = json!(format!("agent/{}", assignment.agent_id));
                }
                item
            })
            .collect();
        page["reports"] = self.reports(&ticket, Page::list(self.read)?);
        without(&mut page, "read");
        Ok(page)
    }

    /// A page of `ticket`'s reports, newest first.
    fn reports(&self, ticket: &Ticket, page: Page) -> Value {
        let newest: Vec<&Report> = ticket.reports.iter().rev().collect();
        let mut reports = page.items(&newest, |report| self.report_item(ticket, report, PREVIEW));
        reports["read"] = json!(self.paths.ticket(ticket.id, "/reports"));
        reports
    }

    /// `ticket/<id>/report/<n>`: one kept report with its text as a text page.
    fn report(&self, id: TicketId, sequence: u64) -> Result<Value, ErrorCode> {
        let ticket = self.full_ticket(id)?;
        let report = ticket
            .reports
            .iter()
            .find(|report| report.sequence == sequence)
            .ok_or(ErrorCode::BotNotFound)?;
        let mut page = Page::text(self.read)?.of_text(&self.read.resource, &report.text);
        page["sequence"] = json!(report.sequence);
        page["at"] = json!(report.at);
        page["source"] = json!(report.source);
        page["kind"] = json!(report.kind);
        page["agentId"] = json!(report.agent_id);
        page["status"] = json!(report.status);
        Ok(page)
    }

    /// Whether the viewer may read the whole of `ticket`.
    fn may_read(&self, ticket: &Ticket) -> bool {
        match self.viewer {
            Viewer::Worker(own) => ticket.id == own,
            Viewer::Owner(_) | Viewer::External(_) => self.viewer.sees(ticket.conversation_id),
        }
    }

    /// Ticket `id` with every kept report, when the viewer may read the whole of it.
    /// # Errors
    /// Returns `BotNotFound` for an unknown ticket and `Forbidden` for one of another
    /// conversation.
    fn full_ticket(&self, id: TicketId) -> Result<Ticket, ErrorCode> {
        let indexed = self.state.indexed_ticket(self.bot.id, id)?;
        if !self.may_read(indexed) {
            return Err(ErrorCode::Forbidden);
        }
        self.state.load_ticket(self.bot.id, id)
    }

    /// `conversations`: the conversations the viewer may read, most recently active first.
    fn conversations(&self) -> Result<Value, ErrorCode> {
        let mut conversations: Vec<&Conversation> = self
            .state
            .conversations
            .values()
            .filter(|conversation| {
                conversation.bot_id == self.bot.id && self.viewer.sees(Some(conversation.id))
            })
            .collect();
        conversations
            .sort_by_key(|conversation| Reverse(self.state.last_activity(conversation.id)));
        Ok(
            Page::list(self.read)?.items(&conversations, |conversation| {
                self.conversation_item(conversation)
            }),
        )
    }

    fn conversation_item(&self, conversation: &Conversation) -> Value {
        let kind = self.state.channel_kind(conversation.channel_id);
        let open = self
            .state
            .tickets
            .values()
            .filter(|ticket| {
                ticket.conversation_id == Some(conversation.id) && !ticket.status.is_closed()
            })
            .count();
        json!({"id":conversation.id,
            "channel":{"id":conversation.channel_id,"kind":kind},
            "label":label(kind.as_deref(), conversation),"peer":conversation.peer,
            "sender":conversation.sender,"thread":conversation.thread,
            "generation":conversation.generation,
            "lastActivityAt":self.state.last_activity(conversation.id),"openTickets":open,
            "thisConversation":self.viewer.conversation() == Some(conversation.id),
            "read":format!("conversation/{}", conversation.id)})
    }

    /// `conversation/<id>`: the conversation and a page of its turns, newest first.
    fn conversation(&self, id: ConversationId) -> Result<Value, ErrorCode> {
        let conversation = self.state.conversation(self.bot.id, id)?;
        if !self.viewer.sees(Some(id)) {
            return Err(ErrorCode::Forbidden);
        }
        let ids: Vec<BotTurnId> = self
            .state
            .order
            .range((id, 0)..=(id, u64::MAX))
            .rev()
            .map(|(_, turn)| *turn)
            .collect();
        let page = Page::list(self.read)?;
        let start = page.offset.min(ids.len());
        let end = start.saturating_add(page.limit).min(ids.len());
        let turns = ids[start..end]
            .iter()
            .map(|turn| self.state.turn(*turn).map(|turn| turn_item(&turn)))
            .collect::<Result<Vec<_>, _>>()?;
        let mut view = self.conversation_item(conversation);
        without(&mut view, "read");
        view["turns"] = json!({"items":turns,"nextOffset":(end < ids.len()).then_some(end)});
        Ok(view)
    }

    /// Turn `id`, when the viewer may read its conversation.
    fn readable_turn(&self, id: BotTurnId) -> Result<Turn, ErrorCode> {
        let turn = self.state.owned_turn(self.bot.id, id)?;
        if !self.viewer.sees(Some(turn.conversation_id)) {
            return Err(ErrorCode::Forbidden);
        }
        Ok(turn)
    }

    /// `turn/<id>`: one turn with its input, a page of its assistant messages and the
    /// notifications that woke it.
    fn turn(&self, id: BotTurnId) -> Result<Value, ErrorCode> {
        let turn = self.readable_turn(id)?;
        let replies = replies(&turn);
        let messages = Page::list(self.read)?.items(&replies, |(index, text, status)| {
            json!({"index":index,"status":status,
                "text":spill(text, DETAIL, || format!("turn/{id}/message/{index}"))})
        });
        let wake = turn.wake.as_ref().map(|wake| {
            wake.notifications
                .iter()
                .map(notification_fields)
                .collect::<Vec<_>>()
        });
        Ok(
            json!({"id":turn.id,"conversation":format!("conversation/{}", turn.conversation_id),
            "sequence":turn.sequence,"generation":turn.generation,"createdAt":turn.created_at,
            "status":turn.status,"delivery":turn.delivery,"error":turn.error,
            "silent":turn.silent,
            "input":spill(&turn.text, DETAIL, || format!("turn/{id}/input")),
            "messages":messages,"wake":wake}),
        )
    }

    /// `notifications`: the log entries the viewer may read, newest first.
    fn notifications(&self) -> Result<Value, ErrorCode> {
        let entries: Vec<&LoggedNotification> = self
            .state
            .notifications
            .get(&self.bot.id)
            .into_iter()
            .flat_map(|log| log.entries.iter().rev())
            .filter(|entry| self.viewer.sees(entry.notification.conversation_id))
            .collect();
        Ok(Page::list(self.read)?.items(&entries, |entry| {
            let mut item = notification_fields(&entry.notification);
            let sequence = entry.sequence;
            item["sequence"] = json!(sequence);
            item["wake"] = self.wake(&entry.wake);
            if let Some(summary) = &entry.notification.summary {
                item["summary"] = spill(summary, PREVIEW, || format!("notification/{sequence}"));
            }
            item["read"] = json!(format!("notification/{sequence}"));
            item
        }))
    }

    /// `notification/<sequence>`: one kept log entry with pointers to what it is about; an entry
    /// the viewer may not read is not kept for it.
    fn notification(&self, sequence: u64) -> Result<Value, ErrorCode> {
        let entry = self
            .state
            .notifications
            .get(&self.bot.id)
            .and_then(|log| log.entries.iter().find(|entry| entry.sequence == sequence))
            .ok_or(ErrorCode::BotNotFound)?;
        let notification = &entry.notification;
        // Sequences are small counters, so another conversation's entry reads as not kept.
        if !self.viewer.sees(notification.conversation_id) {
            return Err(ErrorCode::BotNotFound);
        }
        let mut page = notification_fields(notification);
        page["sequence"] = json!(sequence);
        page["admittedAt"] = json!(entry.admitted_at);
        page["wake"] = self.wake(&entry.wake);
        if let Some(conversation) = notification.conversation_id {
            page["conversation"] = json!(format!("conversation/{conversation}"));
        }
        let report = notification.ticket_id.and_then(|ticket| {
            let rest = notification.id.strip_prefix(&format!("report:{ticket}:"))?;
            rest.parse::<u64>()
                .ok()
                .map(|report| format!("ticket/{ticket}/report/{report}"))
        });
        page["report"] = json!(report);
        Ok(page)
    }

    /// A log entry's wake state, pointing at its wake turn only when the viewer may read it.
    fn wake(&self, wake: &WakeStatus) -> Value {
        match wake {
            WakeStatus::Woke {
                conversation_id,
                turn_id,
            } => {
                let turn = self
                    .viewer
                    .sees(Some(*conversation_id))
                    .then(|| format!("turn/{turn_id}"));
                json!({"state":"woke","turn":turn})
            }
            other => json!(other),
        }
    }

    /// `agent/<id>`: the Agent record the caller read, joined with its ticket and the
    /// conversation it reports to, for a viewer that may see the Agent (ADR-280 §4).
    /// # Errors
    /// Returns `BotNotFound` without the caller's record, and `Forbidden` for an external viewer
    /// when the Agent works for another conversation, the Bot did not start it, or there is no
    /// record: an unknown id answers like someone else's Agent, so the answer does not confirm
    /// which ids exist.
    fn agent(&self, id: &str) -> Result<Value, ErrorCode> {
        let record = self.read.agent.as_ref().filter(|record| record["id"] == id);
        let Some(record) = record else {
            return Err(match self.viewer {
                Viewer::External(_) => ErrorCode::Forbidden,
                Viewer::Owner(_) | Viewer::Worker(_) => ErrorCode::BotNotFound,
            });
        };
        let labels = &record["labels"];
        let seen = self.viewer.sees_agent(labels, |ticket| {
            self.state
                .indexed_ticket(self.bot.id, ticket)
                .ok()
                .map(|ticket| ticket.conversation_id)
        });
        if !seen {
            return Err(ErrorCode::Forbidden);
        }
        let label = |name: &str| labels[name].as_str();
        let mut page = json!({"id":id,"title":record["title"],"status":record["status"],
            "requiresAttention":record["requiresAttention"],"workspaceId":record["workspaceId"],
            "updatedAt":record["updatedAt"],"archived":record["archivedAt"].is_string(),
            "transcript":AGENT_TRANSCRIPT_TOOL,"ticket":null,"latestReport":null,
            "conversation":null});
        let conversation = label(BOT_CONVERSATION_LABEL).and_then(|id| id.parse().ok());
        if self.viewer.sees(conversation)
            && let Some(conversation) = conversation
        {
            page["conversation"] = json!(format!("conversation/{conversation}"));
        }
        let ticket = label(BOT_TICKET_LABEL)
            .and_then(|ticket| ticket.parse::<TicketId>().ok())
            .and_then(|ticket| self.state.indexed_ticket(self.bot.id, ticket).ok());
        if let Some(indexed) = ticket {
            if self.may_read(indexed) {
                let ticket = self.state.load_ticket(self.bot.id, indexed.id)?;
                page["ticket"] = self.ticket_card(&ticket);
                page["latestReport"] = ticket
                    .reports
                    .iter()
                    .rev()
                    .find(|report| report.agent_id.as_deref() == Some(id))
                    .map_or(Value::Null, |report| {
                        self.report_item(&ticket, report, DETAIL)
                    });
            } else {
                page["ticket"] = self.ticket_stub(indexed);
            }
        }
        Ok(page)
    }
}

/// The fields of `notification` every view shows; the free-form id stays off the page
/// because nothing reads by it, and the summary is bounded at admission.
fn notification_fields(notification: &Notification) -> Value {
    let ticket = notification.ticket_id.map(|ticket| {
        json!({"id":ticket,"title":notification.ticket_title,"read":format!("ticket/{ticket}")})
    });
    let mut fields = json!({"kind":notification.kind,"title":notification.title,
        "at":notification.at,"workspaceId":notification.workspace_id,
        "summary":notification.summary,"ticket":ticket});
    let subjects = [
        ("agentId", &notification.agent_id),
        ("scheduleId", &notification.schedule_id),
        ("runId", &notification.run_id),
        ("terminalId", &notification.terminal_id),
    ];
    for (field, id) in subjects {
        if let Some(id) = id {
            fields[field] = json!(id);
        }
    }
    if let Some(agent) = &notification.agent_id {
        fields["agent"] = json!(format!("agent/{agent}"));
    }
    fields
}

/// A turn as an item of its conversation's page, with previews of its input and last reply.
fn turn_item(turn: &Turn) -> Value {
    let id = turn.id;
    let replies = replies(turn);
    let reply = replies
        .last()
        .map(|(index, text, _)| spill(text, PREVIEW, || format!("turn/{id}/message/{index}")));
    json!({"id":id,"sequence":turn.sequence,"generation":turn.generation,
        "createdAt":turn.created_at,"status":turn.status,"delivery":turn.delivery,
        "wake":turn.wake.as_ref().map(|wake| wake.notifications.len()),"silent":turn.silent,
        "input":spill(&turn.text, PREVIEW, || format!("turn/{id}/input")),"reply":reply,
        "read":format!("turn/{id}")})
}

/// The assistant messages of `turn` with their index, text and status; a turn stored before
/// multi-message output has only its legacy reply.
fn replies(turn: &Turn) -> Vec<(usize, &str, Value)> {
    if turn.messages.is_empty() {
        return turn
            .reply
            .as_deref()
            .map(|reply| (0, reply, json!("completed")))
            .into_iter()
            .collect();
    }
    turn.messages
        .iter()
        .enumerate()
        .map(|(index, message)| (index, message.text.as_str(), json!(message.status)))
        .collect()
}

/// A label a person recognizes a conversation by: its channel kind and the chat, sender and
/// thread ids; Ait keeps no display names of external platforms.
fn label(kind: Option<&str>, conversation: &Conversation) -> String {
    let kind = kind.unwrap_or("deleted channel");
    if kind == "builtin" {
        return "Ait".to_owned();
    }
    let mut label = format!("{kind} {}", conversation.peer);
    if conversation.sender != conversation.peer {
        let _ = write!(label, " / {}", conversation.sender);
    }
    if let Some(thread) = &conversation.thread {
        let _ = write!(label, " #{thread}");
    }
    label
}

/// Drop `field` from object `value`; a page's own pointer is not repeated inside it.
fn without(value: &mut Value, field: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(field);
    }
}

#[cfg(test)]
mod tests;
