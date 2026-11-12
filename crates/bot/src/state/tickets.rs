//! Tickets: the units of work a Bot delegates to Agents, with their assignments and reports,
//! attached to the notification inbox (ADR-274).
//!
//! The state keeps an index of every live Bot's tickets holding each ticket's newest report
//! only; a change reads the ticket file, applies the change and writes it back, like turns.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use domain::bot::notification::{Notification, NotificationKind};
use domain::bot::ticket::{
    Report, ReportKind, ReportSource, Ticket, TicketError, TicketStatus, TicketUpdate, WaitReason,
};
use domain::bot::{BotState, Catalog};
use domain::{BotId, TicketId};
use model::ErrorCode;
use model::bot::{StorageError, Store};
use serde_json::{Value, json};

use super::State;
use super::notifications::WAKE_WINDOW;
use crate::protocol::{TicketList, TicketRequest, TicketUserUpdate};
use crate::tickets::{AssignTicket, OpenTicket, OpenedTicket, WorkerReport};

/// Most tickets that are not closed per Bot; opening or reopening one more is refused.
const OPEN_LIMIT: usize = 32;
/// Most closed tickets kept per Bot; the earliest closed are deleted first.
const CLOSED_LIMIT: usize = 64;
/// Outcome and system report of an open ticket Ait closed to make room under [`OPEN_LIMIT`].
const CLOSED_FINISHED: &str = "Closed automatically: every Agent working on it was archived and \
    the Bot had reached its open ticket limit";
/// Outcome and system report of a ticket waiting on a blocked worker that Ait cancelled to make
/// room under [`OPEN_LIMIT`].
const CLOSED_BLOCKED: &str = "Cancelled automatically: it waited on a worker that reported it \
    was blocked, every Agent working on it was archived so nobody can continue it, and the Bot \
    had reached its open ticket limit";
/// Outcome and system report of a ticket waiting on a permission request that Ait cancelled to
/// make room under [`OPEN_LIMIT`].
const CLOSED_PERMISSION: &str = "Cancelled automatically: it waited on a permission request, \
    every Agent working on it was archived so nobody can answer it, and the Bot had reached its \
    open ticket limit";
/// Default and largest page of `bot.ticket.list.request`.
const LIST_DEFAULT: usize = 50;
const LIST_LIMIT: usize = 128;

/// The ticket index of the catalog's live Bots, and the wake-up of every worker outcome report
/// newer than [`WAKE_WINDOW`] at `now`, to offer again once the inbox is loaded.
pub(super) struct Loaded {
    pub(super) tickets: BTreeMap<TicketId, Ticket>,
    pub(super) reports: Vec<Notification>,
}

/// Load the ticket index of the catalog's live Bots, deleting the tickets of retired or
/// unknown Bots that a crash while retiring left behind, and rebuild the wake-ups of recent
/// worker outcome reports: a crash between saving a report and logging its notification would
/// otherwise lose the wake-up, and the log and the recent-wake record drop the ones it holds.
pub(super) fn load(
    store: &dyn Store,
    catalog: &Catalog,
    now: DateTime<Utc>,
) -> Result<Loaded, ErrorCode> {
    let live: BTreeMap<BotId, &str> = catalog
        .bots
        .iter()
        .filter(|bot| !matches!(bot.state, BotState::Retired { .. }))
        .map(|bot| (bot.id, bot.workspace_id.as_str()))
        .collect();
    let mut tickets = BTreeMap::new();
    let mut reports = Vec::new();
    let mut stale = BTreeSet::new();
    store
        .scan_tickets(&mut |ticket| {
            let Some(scope) = live.get(&ticket.bot_id) else {
                stale.insert(ticket.bot_id);
                return Ok(());
            };
            reports.extend(
                ticket
                    .reports
                    .iter()
                    .filter(|report| now - report.at < WAKE_WINDOW)
                    .filter_map(|report| report_notification(&ticket, report, scope)),
            );
            if tickets.insert(ticket.id, indexed(ticket)).is_some() {
                return Err(StorageError);
            }
            Ok(())
        })
        .map_err(|_| ErrorCode::BotIo)?;
    for bot in stale {
        store.delete_tickets(bot).map_err(|_| ErrorCode::BotIo)?;
    }
    Ok(Loaded { tickets, reports })
}

impl State {
    /// Open a ticket for the first Agent a Bot session starts for a task, or return the one an
    /// earlier attempt with the same key opened.
    pub(crate) fn open_ticket(
        &mut self,
        request: OpenTicket,
        now: DateTime<Utc>,
    ) -> Result<OpenedTicket, ErrorCode> {
        let bot = self.ticket_bot(&request.root)?;
        if let Some(conversation) = request.draft.conversation_id {
            self.conversation(bot, conversation)
                .map_err(|_| ErrorCode::InvalidMessage)?;
        }
        let key = Some(request.draft.key.as_str());
        if let Some(existing) = self
            .tickets
            .values()
            .find(|ticket| ticket.bot_id == bot && ticket.key.as_deref() == key)
        {
            if !existing.opened_with(&request.draft) {
                return Err(ErrorCode::IdempotencyConflict);
            }
            return Ok(OpenedTicket {
                ticket_id: existing.id,
                created: false,
            });
        }
        if self.open_tickets(bot) >= OPEN_LIMIT {
            return Err(ErrorCode::ResourceExhausted);
        }
        let ticket =
            Ticket::open(TicketId::generate(), bot, request.draft, now).map_err(refused)?;
        let ticket_id = ticket.id;
        self.save_ticket(ticket)?;
        Ok(OpenedTicket {
            ticket_id,
            created: true,
        })
    }

    /// Record that an Agent works on a ticket; assigning it again changes nothing.
    pub(crate) fn assign_ticket(
        &mut self,
        request: &AssignTicket,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        if !model::valid_id(&request.agent_id) || !model::valid_id(&request.workspace_id) {
            return Err(ErrorCode::InvalidMessage);
        }
        let bot = self.ticket_bot(&request.root)?;
        let mut ticket = self.load_ticket(bot, request.ticket_id)?;
        if ticket
            .assign(&request.agent_id, &request.workspace_id, now)
            .map_err(refused)?
        {
            self.save_ticket(ticket)?;
        }
        Ok(())
    }

    /// Delete a ticket whose first Agent could not be created; one with assignments stays.
    pub(crate) fn abandon_ticket(&mut self, root: &str, id: TicketId) -> Result<(), ErrorCode> {
        let bot = self.ticket_bot(root)?;
        if self.indexed_ticket(bot, id)?.assignments.is_empty() {
            self.delete_ticket(bot, id)?;
        }
        Ok(())
    }

    /// Record the report of an Agent whose label names the ticket, assigning the Agent first
    /// while the ticket has room, and, unless it only reports progress, wake the Bot in the
    /// ticket's conversation through the inbox. Returns the ticket's status after it.
    pub(crate) fn report_progress(
        &mut self,
        report: &WorkerReport,
        now: DateTime<Utc>,
    ) -> Result<TicketStatus, ErrorCode> {
        if !model::valid_id(&report.agent_id) || !model::valid_id(&report.workspace_id) {
            return Err(ErrorCode::InvalidMessage);
        }
        let bot = self.ticket_bot(&report.root)?;
        let scope = self.bot(bot)?.workspace_id.clone();
        let mut ticket = self.load_ticket(bot, report.ticket_id)?;
        // The daemon-written label is the authorization; a full ticket still hears the Agent.
        match ticket.assign(&report.agent_id, &report.workspace_id, now) {
            Ok(_) | Err(TicketError::Full) => {}
            Err(error) => return Err(refused(error)),
        }
        ticket
            .report(&report.agent_id, report.kind, &report.text, now)
            .map_err(refused)?;
        let status = ticket.status;
        let notification = ticket
            .reports
            .last()
            .and_then(|recorded| report_notification(&ticket, recorded, &scope));
        // The report is durable before the wake-up; a crash in between keeps the report, and
        // loading offers its wake-up again.
        self.save_ticket(ticket)?;
        if let Some(notification) = notification {
            self.notify(notification, now)?;
        }
        Ok(status)
    }

    /// Offer again the wake-ups [`load`] rebuilt from recent worker reports; those already
    /// logged or woken are dropped as duplicates.
    pub(super) fn offer_reports(
        &mut self,
        reports: Vec<Notification>,
        now: DateTime<Utc>,
    ) -> Result<(), ErrorCode> {
        for notification in reports {
            if let Err(error) = self.notify(notification, now) {
                if self.frozen {
                    return Err(error);
                }
                tracing::warn!(?error, "worker report wake-up was not offered again");
            }
        }
        Ok(())
    }

    /// Close a ticket of `root`'s Bot whose Agents are all archived, as the caller judged, on
    /// Ait's behalf (ADR-274 section 2), with an outcome that says why; a waiting ticket is
    /// cancelled, and closed and Agent-less tickets stay. Returns the new status, if any.
    pub(crate) fn close_finished_ticket(
        &mut self,
        root: &str,
        id: TicketId,
        now: DateTime<Utc>,
    ) -> Result<Option<TicketStatus>, ErrorCode> {
        let bot = self.ticket_bot(root)?;
        let mut ticket = self.load_ticket(bot, id)?;
        let reason = match ticket.waiting.as_ref().map(|waiting| waiting.reason) {
            None => CLOSED_FINISHED,
            Some(WaitReason::Blocked) => CLOSED_BLOCKED,
            Some(WaitReason::Permission) => CLOSED_PERMISSION,
        };
        let Some(status) = ticket.close_finished(reason, now) else {
            return Ok(None);
        };
        self.save_ticket(ticket)?;
        self.evict_closed(bot)?;
        Ok(Some(status))
    }

    /// Reopen a closed ticket of `root`'s Bot because the Bot gave `agent` more work; `false`
    /// when it is not closed.
    pub(crate) fn reopen_ticket(
        &mut self,
        root: &str,
        id: TicketId,
        agent: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, ErrorCode> {
        let bot = self.ticket_bot(root)?;
        if !self.indexed_ticket(bot, id)?.status.is_closed() {
            return Ok(false);
        }
        if self.open_tickets(bot) >= OPEN_LIMIT {
            return Err(ErrorCode::ResourceExhausted);
        }
        let mut ticket = self.load_ticket(bot, id)?;
        let reason = format!("Reopened automatically: the Bot gave Agent {agent} more work");
        if !ticket.reopen(&reason, now) {
            return Ok(false);
        }
        self.save_ticket(ticket)?;
        Ok(true)
    }

    /// Apply the Bot's own update to one of its tickets.
    pub(crate) fn update_ticket(
        &mut self,
        root: &str,
        id: TicketId,
        update: &TicketUpdate,
        now: DateTime<Utc>,
    ) -> Result<Value, ErrorCode> {
        let bot = self.ticket_bot(root)?;
        self.change_ticket(bot, id, ReportSource::Bot, update, now)
    }

    /// Give `notification`, which `bot` is admitting, the ticket it names if `bot` owns it, and
    /// record the system report about the ticket's Agent; a ticket of another Bot is dropped
    /// from `bot`'s copy. The flag is set when [`Ticket::spares_turn_end`] finds that the end of
    /// an Agent turn only repeats the outcome report that already woke the Bot. A ticket that
    /// cannot be read is logged and the notification admitted without its report.
    pub(super) fn attach_ticket(
        &mut self,
        bot: BotId,
        notification: &Notification,
        now: DateTime<Utc>,
    ) -> Result<(Notification, bool), ErrorCode> {
        let mut notification = notification.clone();
        let owned = notification
            .ticket_id
            .and_then(|id| self.indexed_ticket(bot, id).ok())
            .map(|ticket| (ticket.id, ticket.title.clone()));
        let Some((id, title)) = owned else {
            notification.ticket_id = None;
            notification.ticket_title = None;
            return Ok((notification, false));
        };
        notification.ticket_title = Some(title);
        if !matches!(
            notification.kind,
            NotificationKind::AgentFinished
                | NotificationKind::AgentFailed
                | NotificationKind::AgentPermission
        ) {
            return Ok((notification, false));
        }
        let mut ticket = match self.load_ticket(bot, id) {
            Ok(ticket) => ticket,
            Err(error) => {
                tracing::warn!(%bot, ticket = %id, ?error, "Bot ticket unreadable; notification admitted without its report");
                return Ok((notification, false));
            }
        };
        let reported = ticket.spares_turn_end(&notification);
        ticket.record_notification(&notification, now);
        self.save_ticket(ticket)?;
        Ok((notification, reported))
    }

    /// Remove every ticket of retired `bot`.
    pub(super) fn delete_tickets(&mut self, bot: BotId) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.delete_tickets(bot).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.tickets.retain(|_, ticket| ticket.bot_id != bot);
        Ok(())
    }

    /// `bot.ticket.list.request`: open tickets, most recently changed first, then closed ones,
    /// most recently closed first, when asked for.
    pub(super) fn list_tickets(&self, request: &TicketList) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        let limit = request.limit.unwrap_or(LIST_DEFAULT);
        if limit == 0 || limit > LIST_LIMIT {
            return Err(ErrorCode::InvalidMessage);
        }
        let (mut closed, mut open): (Vec<&Ticket>, Vec<&Ticket>) = self
            .tickets
            .values()
            .filter(|ticket| ticket.bot_id == bot)
            .partition(|ticket| ticket.status.is_closed());
        open.sort_unstable_by_key(|ticket| Reverse(ticket.updated_at));
        if request.include_closed {
            closed.sort_unstable_by_key(|ticket| Reverse(ticket.closed_at));
        } else {
            closed.clear();
        }
        let tickets: Vec<Value> = open
            .into_iter()
            .chain(closed)
            .take(limit)
            .map(summary_view)
            .collect();
        Ok(json!({"tickets":tickets}))
    }

    /// `bot.ticket.get.request`: one ticket with its request and reports.
    pub(super) fn get_ticket(&self, request: &TicketRequest) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        self.indexed_ticket(bot, request.ticket_id)?;
        let ticket = self
            .store
            .ticket(bot, request.ticket_id)
            .map_err(|_| ErrorCode::BotIo)?;
        Ok(json!({"ticket":full_view(&ticket)}))
    }

    /// `bot.ticket.update.request`: the user closes, cancels or reopens a ticket, also while
    /// the Bot is paused.
    pub(super) fn user_update_ticket(
        &mut self,
        request: TicketUserUpdate,
        now: DateTime<Utc>,
    ) -> Result<Value, ErrorCode> {
        let bot = self.current(&request.workspace_id)?.id;
        let update = TicketUpdate {
            status: Some(request.status),
            outcome: request.outcome,
            ..TicketUpdate::default()
        };
        self.change_ticket(bot, request.ticket_id, ReportSource::User, &update, now)
    }

    fn change_ticket(
        &mut self,
        bot: BotId,
        id: TicketId,
        source: ReportSource,
        update: &TicketUpdate,
        now: DateTime<Utc>,
    ) -> Result<Value, ErrorCode> {
        let mut ticket = self.load_ticket(bot, id)?;
        let reopening = update.status == Some(TicketStatus::Open) && ticket.status.is_closed();
        if reopening && self.open_tickets(bot) >= OPEN_LIMIT {
            return Err(ErrorCode::ResourceExhausted);
        }
        let changed = ticket.update(source, update, now).map_err(refused)?;
        let view = full_view(&ticket);
        if changed {
            let closed = ticket.status.is_closed();
            self.save_ticket(ticket)?;
            if closed {
                self.evict_closed(bot)?;
            }
        }
        Ok(json!({"ticket":view,"changed":changed}))
    }

    /// The id of `root`'s current Bot, which must be active to work on tickets.
    fn ticket_bot(&self, root: &str) -> Result<BotId, ErrorCode> {
        let bot = self.current(root)?;
        if !matches!(bot.state, BotState::Active) {
            return Err(ErrorCode::BotConflict);
        }
        Ok(bot.id)
    }

    fn indexed_ticket(&self, bot: BotId, id: TicketId) -> Result<&Ticket, ErrorCode> {
        self.tickets
            .get(&id)
            .filter(|ticket| ticket.bot_id == bot)
            .ok_or(ErrorCode::BotNotFound)
    }

    /// Read `bot`'s ticket `id` with all its reports.
    fn load_ticket(&self, bot: BotId, id: TicketId) -> Result<Ticket, ErrorCode> {
        self.indexed_ticket(bot, id)?;
        self.store.ticket(bot, id).map_err(|_| ErrorCode::BotIo)
    }

    fn open_tickets(&self, bot: BotId) -> usize {
        self.tickets
            .values()
            .filter(|ticket| ticket.bot_id == bot && !ticket.status.is_closed())
            .count()
    }

    fn save_ticket(&mut self, ticket: Ticket) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.save_ticket(&ticket).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.tickets.insert(ticket.id, indexed(ticket));
        Ok(())
    }

    fn delete_ticket(&mut self, bot: BotId, id: TicketId) -> Result<(), ErrorCode> {
        self.ensure_writable()?;
        if self.store.delete_ticket(bot, id).is_err() {
            self.frozen = true;
            return Err(ErrorCode::BotIo);
        }
        self.tickets.remove(&id);
        Ok(())
    }

    /// Delete `bot`'s earliest closed tickets beyond [`CLOSED_LIMIT`].
    fn evict_closed(&mut self, bot: BotId) -> Result<(), ErrorCode> {
        let mut closed: Vec<(Option<DateTime<Utc>>, TicketId)> = self
            .tickets
            .values()
            .filter(|ticket| ticket.bot_id == bot && ticket.status.is_closed())
            .map(|ticket| (ticket.closed_at, ticket.id))
            .collect();
        let excess = closed.len().saturating_sub(CLOSED_LIMIT);
        if excess == 0 {
            return Ok(());
        }
        closed.select_nth_unstable(excess - 1);
        closed[..excess]
            .iter()
            .try_for_each(|(_, id)| self.delete_ticket(bot, *id))
    }
}

/// `ticket` as the index keeps it: everything but its reports, of which only the newest.
fn indexed(mut ticket: Ticket) -> Ticket {
    let older = ticket.reports.len().saturating_sub(1);
    ticket.reports.drain(..older);
    ticket
}

/// The inbox notification for `report` of `ticket`, whose Bot's root Workspace is `scope`:
/// worker `blocked`, `done` and `failed` reports wake the Bot; others do not.
fn report_notification(ticket: &Ticket, report: &Report, scope: &str) -> Option<Notification> {
    if report.source != ReportSource::Agent {
        return None;
    }
    let kind = match report.kind {
        ReportKind::Blocked => NotificationKind::TicketBlocked,
        ReportKind::Done => NotificationKind::TicketDone,
        ReportKind::Failed => NotificationKind::TicketFailed,
        ReportKind::Progress
        | ReportKind::AgentFinished
        | ReportKind::AgentFailed
        | ReportKind::AgentPermission
        | ReportKind::Note
        | ReportKind::Status => return None,
    };
    let agent = report.agent_id.as_deref()?;
    let workspace_id = ticket
        .assignments
        .iter()
        .find(|assignment| assignment.agent_id == agent)
        .map_or(scope, |assignment| assignment.workspace_id.as_str());
    Some(Notification {
        id: format!("report:{}:{}", ticket.id, report.sequence),
        workspace_id: workspace_id.to_owned(),
        scope: Some(scope.to_owned()),
        kind,
        agent_id: Some(agent.to_owned()),
        schedule_id: None,
        run_id: None,
        terminal_id: None,
        title: ticket.title.clone(),
        summary: Some(report.text.clone()).filter(|text| !text.is_empty()),
        at: report.at,
        conversation_id: ticket.conversation_id,
        ticket_id: Some(ticket.id),
        ticket_title: Some(ticket.title.clone()),
        turn_started_at: None,
    })
}

/// Client projection shared by summaries and full tickets, without the Bot or the key.
fn view(ticket: &Ticket) -> Value {
    json!({"id":ticket.id,"title":ticket.title,"status":ticket.status,"waiting":ticket.waiting,
        "conversationId":ticket.conversation_id,"createdAt":ticket.created_at,
        "updatedAt":ticket.updated_at,"closedAt":ticket.closed_at,"outcome":ticket.outcome,
        "assignments":ticket.assignments})
}

/// List projection: the shared fields and the newest report.
fn summary_view(ticket: &Ticket) -> Value {
    let mut value = view(ticket);
    value["latestReport"] = json!(ticket.reports.last());
    value
}

/// Full projection: the shared fields, the request and every kept report.
fn full_view(ticket: &Ticket) -> Value {
    let mut value = view(ticket);
    value["request"] = json!(ticket.request);
    value["reports"] = json!(ticket.reports);
    value
}

/// The error code of a refused ticket change.
fn refused(error: TicketError) -> ErrorCode {
    match error {
        TicketError::Invalid => ErrorCode::InvalidMessage,
        TicketError::Closed | TicketError::Conflict => ErrorCode::BotConflict,
        TicketError::Full => ErrorCode::ResourceExhausted,
    }
}

#[cfg(test)]
mod tests;
