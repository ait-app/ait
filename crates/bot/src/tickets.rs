//! Requests through which the scoped MCP opens, assigns and reports on a Bot's tickets
//! (ADR-274, ADR-287); see [`crate::service::Bots`].

use domain::TicketId;
use domain::bot::ticket::{TicketDraft, WorkerReportKind};
use domain::forge::PullSnapshot;
use serde::{Deserialize, Serialize};

/// Tool error for a refused [`crate::service::Bots::open_ticket`] or reopening at the open
/// ticket limit, which reports it as `ResourceExhausted`, once no ticket could be closed
/// automatically.
pub const OPEN_LIMIT_MESSAGE: &str = "Too many open tickets, and none could be closed \
    automatically. Call bot_ticket_list and close the tickets whose work is finished or no longer \
    needed with bot_ticket_update (status done or cancelled), then try again";

/// Tool error for binding a pull request beyond the per-ticket or per-Workspace bound, which
/// the report or update returns as `ResourceExhausted`.
pub const PULL_LIMIT_MESSAGE: &str = "This ticket or Workspace already follows the most pull \
    requests it can (4 per ticket, 16 per Workspace); the report was not recorded. Report \
    without pullRequest. Closed tickets keep following open pull requests and, for 7 days, \
    merged ones; the Bot frees a place by setting such a done or failed ticket to cancelled with \
    bot_ticket_update, which stops following its pull requests";

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
    /// The one question a `blocked` report asks the user (ADR-287).
    pub question: Option<String>,
    /// A pull request to follow, already read with [`crate::service::Bots::read_pull`].
    pub pull: Option<PullSnapshot>,
}
