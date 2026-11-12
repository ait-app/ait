//! Requests through which the scoped MCP opens, assigns and reports on a Bot's tickets
//! (ADR-274); see [`crate::service::Bots`].

use domain::TicketId;
use domain::bot::ticket::{TicketDraft, WorkerReportKind};
use serde::{Deserialize, Serialize};

/// Tool error for a refused [`crate::service::Bots::open_ticket`] or reopening at the open
/// ticket limit, which reports it as `ResourceExhausted`, once no ticket could be closed
/// automatically.
pub const OPEN_LIMIT_MESSAGE: &str = "Too many open tickets, and none could be closed \
    automatically. Call bot_tickets and close the tickets whose work is finished or no longer \
    needed with bot_ticket_update (status done or cancelled), then try again";

/// Open a ticket for the first Agent a Bot session starts for a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTicket {
    /// Root Workspace of the calling Bot.
    pub root: String,
    /// The ticket; its key is the Agent creation's idempotency key.
    pub draft: TicketDraft,
}

/// The ticket [`OpenTicket`] resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedTicket {
    /// The ticket the Agent belongs to.
    pub ticket_id: TicketId,
    /// `false` when an earlier attempt with the same key already opened it.
    pub created: bool,
}

/// Record that an Agent works on a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignTicket {
    /// Root Workspace of the calling Bot or subtree Agent.
    pub root: String,
    /// The ticket, from the Bot's `ticketId` or the parent Agent's `ait.bot.ticket` label.
    pub ticket_id: TicketId,
    /// The new Agent.
    pub agent_id: String,
    /// Workspace the new Agent runs in.
    pub workspace_id: String,
}

/// What an Agent reports about the ticket its `ait.bot.ticket` label names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerReport {
    /// Root Workspace of the calling subtree Agent.
    pub root: String,
    /// The ticket from the Agent's label.
    pub ticket_id: TicketId,
    /// The reporting Agent.
    pub agent_id: String,
    /// Workspace the reporting Agent runs in, for assigning it when it is not yet assigned.
    pub workspace_id: String,
    /// What it reports.
    pub kind: WorkerReportKind,
    /// Its text; truncated to the report bound.
    pub text: String,
}
