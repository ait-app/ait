//! Units of work a Bot delegates to Agents: tickets, the Agents assigned to them and their
//! reports, with the ticket status machine (ADR-274).
//!
//! These records are new, so they accept unknown fields: a later daemon can extend them without
//! making an earlier one refuse the Bot store.

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

use crate::bot::follow::{
    BoundBy, Delivery, Lookups, PULL_LIMIT, PullBinding, PullEvent, bot_title, data, outside_text,
    worker_text,
};
use crate::bot::notification::{Notification, NotificationKind};
use crate::forge::{PullSnapshot, PullState};
use crate::input_envelope;
use crate::{BotId, ConversationId, TicketId};

/// Longest kept ticket title in bytes; longer titles are truncated.
const TITLE_LIMIT: usize = 256;
/// Longest kept ticket request in bytes; longer requests are truncated.
const REQUEST_LIMIT: usize = 4096;
/// Longest kept report text or outcome in bytes; longer texts are truncated.
const TEXT_LIMIT: usize = 2048;
/// Longest idempotency key in bytes.
const KEY_LIMIT: usize = 256;
/// Most Agents assigned to one ticket; the scoped MCP refuses to start another Agent on a full
/// ticket.
pub const ASSIGNMENT_LIMIT: usize = 16;
/// Most reports one ticket keeps; the oldest go first.
const REPORT_LIMIT: usize = 32;
/// Longest question a blocked worker asks, in bytes.
const QUESTION_LIMIT: usize = 512;
/// Most pull request inputs waiting for delivery per ticket; beyond it they go to the Bot.
pub const PENDING_LIMIT: usize = 16;
/// How long a ticket its pull requests ended keeps following them, so a later comment can
/// resume it (ADR-287).
pub const FOLLOW_AFTER: TimeDelta = TimeDelta::days(7);
/// Prefix of the ids of pull request inputs; answers use `answer:`.
const PULL_INPUT: &str = "follow:";
/// Failed attempts after which an input for a live Agent goes to the Bot instead, once they
/// also span [`DELIVERY_FAILURE_WINDOW`].
const DELIVERY_FAILURE_LIMIT: u32 = 8;
/// How long an input's failed attempts must go on, each within this long of the one before
/// and with no busy reply between, before it goes to the Bot; comfortably longer than a
/// provider restart (ADR-287).
const DELIVERY_FAILURE_WINDOW: TimeDelta = TimeDelta::minutes(10);
/// How long an unclosed ticket without assignments may wait for its first Agent before Ait may
/// remove it as left behind by a crash (ADR-274 section 6): well past the five minutes a
/// detached scoped `create_agent` may take between opening the ticket and creating the Agent.
pub const ORPHAN_GRACE: TimeDelta = TimeDelta::hours(1);

/// Where a ticket stands; `done`, `failed` and `cancelled` are closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    /// Work is in progress.
    Open,
    /// Work waits on the user: an Agent needs a permission decision or reported it is blocked.
    Waiting,
    /// Closed with the work done.
    Done,
    /// Closed with the work failed.
    Failed,
    /// Closed without finishing the work.
    Cancelled,
}

impl TicketStatus {
    /// Whether the ticket is closed; only the Bot or the user reopens it.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

/// Why a ticket waits on the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitReason {
    /// The Agent waits for a permission decision in Ait.
    Permission,
    /// The Agent reported it cannot continue without the user.
    Blocked,
}

/// What a waiting ticket waits for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    /// Cause.
    pub reason: WaitReason,
    /// Agent whose event made the ticket wait; only its later events end the wait.
    pub agent_id: String,
    /// When the ticket started waiting.
    pub since: DateTime<Utc>,
    /// The one question a blocked worker asked the user (ADR-287).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

/// The durable link from a ticket to an Agent working on it; the Agent's live state is read
/// from the Agent directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    /// Assigned Agent.
    pub agent_id: String,
    /// Workspace the Agent runs in.
    pub workspace_id: String,
    /// When the Agent was assigned.
    pub assigned_at: DateTime<Utc>,
}

/// Who wrote a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportSource {
    /// An assigned Agent, through its worker tool.
    Agent,
    /// The Bot, through its ticket tool.
    Bot,
    /// The user, through a client.
    User,
    /// Ait, from a Workspace notification about an Agent.
    System,
}

/// What a report says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    /// A worker's progress.
    Progress,
    /// A worker cannot continue without the user.
    Blocked,
    /// A worker finished its part.
    Done,
    /// A worker gave up on its part.
    Failed,
    /// An assigned Agent finished a turn.
    AgentFinished,
    /// An assigned Agent's turn failed.
    AgentFailed,
    /// An assigned Agent waits for a permission decision.
    AgentPermission,
    /// A note the Bot added.
    Note,
    /// The Bot or the user changed the status.
    Status,
    /// A pull request was bound, or a followed one changed (ADR-287).
    Pull,
    /// The user, or the Bot for them, answered a blocked worker's question.
    Answer,
}

/// What an Agent may report about its ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerReportKind {
    /// Progress; never wakes the Bot.
    Progress,
    /// Cannot continue without the user; makes an open ticket wait.
    Blocked,
    /// Finished this Agent's part; the ticket stays open until the Bot closes it.
    Done,
    /// Gave up on this Agent's part; the ticket stays open until the Bot closes it.
    Failed,
}

impl WorkerReportKind {
    /// The notification that wakes the Bot for this report, if any.
    #[must_use]
    pub const fn notification_kind(self) -> Option<NotificationKind> {
        match self {
            Self::Progress => None,
            Self::Blocked => Some(NotificationKind::TicketBlocked),
            Self::Done => Some(NotificationKind::TicketDone),
            Self::Failed => Some(NotificationKind::TicketFailed),
        }
    }

    const fn report_kind(self) -> ReportKind {
        match self {
            Self::Progress => ReportKind::Progress,
            Self::Blocked => ReportKind::Blocked,
            Self::Done => ReportKind::Done,
            Self::Failed => ReportKind::Failed,
        }
    }
}

/// One dated entry in a ticket's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Position in the ticket's history, increasing from 1.
    pub sequence: u64,
    /// When it was written.
    pub at: DateTime<Utc>,
    /// Author.
    pub source: ReportSource,
    /// Meaning.
    pub kind: ReportKind,
    /// Agent the report comes from or is about.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Bounded human text.
    #[serde(default)]
    pub text: String,
    /// The status this report moved the ticket to, if it changed it.
    #[serde(default)]
    pub status: Option<TicketStatus>,
}

/// What a ticket is opened with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketDraft {
    /// Bot conversation the work was asked in.
    pub conversation_id: Option<ConversationId>,
    /// Idempotency key of the Agent creation that opens the ticket.
    pub key: String,
    /// Short title, such as the first Agent's title.
    pub title: String,
    /// What was asked, such as the first Agent's prompt.
    pub request: String,
}

/// A change the Bot or the user asks for; at least one field is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TicketUpdate {
    /// New status: `open` reopens or ends a wait; `done`, `failed` or `cancelled` close.
    #[serde(default)]
    pub status: Option<TicketStatus>,
    /// Conclusion, only with a closing status.
    #[serde(default)]
    pub outcome: Option<String>,
    /// New title.
    #[serde(default)]
    pub title: Option<String>,
    /// A note appended to the history.
    #[serde(default)]
    pub note: Option<String>,
    /// An answer to the question of the worker the ticket waits on, delivered to it.
    #[serde(default)]
    pub answer: Option<Answer>,
    /// A pull request to follow, already read and validated by the caller.
    #[serde(skip)]
    pub pull: Option<PullSnapshot>,
}

/// An answer to a blocked worker's question, naming the worker it answers (ADR-287).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Answer {
    /// The blocked worker whose question this answers; the ticket must still wait on it.
    pub agent_id: String,
    /// The answer.
    pub text: String,
}

/// A worker turn that a delivered pull request input started, until a turn end of that worker
/// after the send is recorded; that turn's end does not wake the Bot (ADR-287).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputTurn {
    /// The worker.
    pub agent_id: String,
    /// When the follower sent the input to the idle worker, before the turn it started began;
    /// absent on records of earlier builds, whose turns are never spared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<DateTime<Utc>>,
    /// When the provider accepted the input, after the turn it started began.
    pub at: DateTime<Utc>,
}

/// A refused ticket change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TicketError {
    /// The input is malformed or empty.
    #[error("invalid ticket input")]
    Invalid,
    /// The ticket is closed.
    #[error("the ticket is closed")]
    Closed,
    /// The status does not allow the change.
    #[error("the ticket status does not allow this change")]
    Conflict,
    /// The ticket already has the most assignments it may have.
    #[error("the ticket has too many assignments")]
    Full,
}

/// A unit of work one Bot delegated to Agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ticket {
    /// Stable identity.
    pub id: TicketId,
    /// Owning Bot.
    pub bot_id: BotId,
    /// Short title.
    pub title: String,
    /// What was asked.
    pub request: String,
    /// Bot conversation the work was asked in.
    #[serde(default)]
    pub conversation_id: Option<ConversationId>,
    /// Current status.
    pub status: TicketStatus,
    /// What the ticket waits for; set exactly while `status` is `waiting`.
    #[serde(default)]
    pub waiting: Option<Waiting>,
    /// Workers whose latest report says they are blocked, oldest first, at most
    /// [`ASSIGNMENT_LIMIT`]; the ticket waits on the first of them while any remain.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<Waiting>,
    /// When the ticket was opened.
    pub created_at: DateTime<Utc>,
    /// When the ticket last changed.
    pub updated_at: DateTime<Utc>,
    /// When the ticket was closed; absent while open.
    #[serde(default)]
    pub closed_at: Option<DateTime<Utc>>,
    /// Conclusion recorded when it was closed.
    #[serde(default)]
    pub outcome: Option<String>,
    /// Idempotency key of the Agent creation that opened it.
    #[serde(default)]
    pub key: Option<String>,
    /// Agents working on it, in assignment order.
    #[serde(default)]
    pub assignments: Vec<Assignment>,
    /// Sequence the next report receives.
    pub next_report: u64,
    /// The newest reports, oldest first.
    #[serde(default)]
    pub reports: Vec<Report>,
    /// Pull requests the ticket follows, at most [`PULL_LIMIT`] (ADR-287).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pulls: Vec<PullBinding>,
    /// Inputs for the ticket's Agents not yet delivered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<Delivery>,
    /// Until when a ticket its pull requests ended keeps following them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_until: Option<DateTime<Utc>>,
    /// Worker turns that delivered inputs started, one per worker, until their end.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_turns: Vec<InputTurn>,
}

/// What one read of a followed pull request asks of the ticket's owner (ADR-287).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Followed {
    /// Notifications for the Bot, which the caller offers before saving the ticket.
    pub notices: Vec<PullNotice>,
    /// Whether the ticket changed and must be saved.
    pub changed: bool,
    /// Whether the read closed the ticket.
    pub closed: bool,
}

/// One notification a followed pull request raises for the Bot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullNotice {
    /// Deterministic identity.
    pub id: String,
    /// Kind.
    pub kind: NotificationKind,
    /// Worker that bound the pull request, if any.
    pub agent_id: Option<String>,
    /// Short title.
    pub title: String,
    /// What happened.
    pub summary: Option<String>,
}

/// What recording a failed read of a followed pull request did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullFailure {
    /// Whether the ticket stopped following the pull request, which the forge kept reporting
    /// missing.
    pub dropped: bool,
    /// The notice that wakes the Bot about it, only for a ticket that is not closed.
    pub notices: Vec<PullNotice>,
}

/// One read of a followed pull request, with what the follower learned around it.
#[derive(Debug, Clone, Copy)]
pub struct PullObservation<'a> {
    /// The read.
    pub snapshot: &'a PullSnapshot,
    /// Entries a live worker of the ticket wrote itself.
    pub authored: &'a BTreeSet<String>,
    /// The write-access lookups the follower made for this read.
    pub lookups: &'a Lookups,
    /// Unarchived Agents carrying the ticket's label.
    pub live: &'a [String],
    /// Whether the ticket may reopen if it was closed; false at the open ticket limit.
    pub reopen: bool,
    /// When it was read.
    pub at: DateTime<Utc>,
}

impl Ticket {
    /// Open ticket `id` of `bot_id` from `draft` at `at`, bounding its texts.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Invalid`] for a blank title or an empty, oversized or
    /// control-character key.
    pub fn open(
        id: TicketId,
        bot_id: BotId,
        draft: TicketDraft,
        at: DateTime<Utc>,
    ) -> Result<Self, TicketError> {
        let key = &draft.key;
        if key.is_empty() || key.len() > KEY_LIMIT || key.chars().any(char::is_control) {
            return Err(TicketError::Invalid);
        }
        let title = valid_title(&draft.title)?.to_owned();
        let mut request = draft.request;
        request.truncate(clip(&request, REQUEST_LIMIT).len());
        Ok(Self {
            id,
            bot_id,
            title,
            request,
            conversation_id: draft.conversation_id,
            status: TicketStatus::Open,
            waiting: None,
            blocked: Vec::new(),
            created_at: at,
            updated_at: at,
            closed_at: None,
            outcome: None,
            key: Some(draft.key),
            assignments: Vec::new(),
            next_report: 1,
            reports: Vec::new(),
            pulls: Vec::new(),
            pending: Vec::new(),
            follow_until: None,
            input_turns: Vec::new(),
        })
    }

    /// Whether this ticket was opened with what `draft` asks for, after bounding.
    #[must_use]
    pub fn opened_with(&self, draft: &TicketDraft) -> bool {
        self.conversation_id == draft.conversation_id
            && self.title == clip(draft.title.trim(), TITLE_LIMIT)
            && self.request == clip(&draft.request, REQUEST_LIMIT)
    }

    /// Assign `agent_id`, running in `workspace_id`, at `at`; `false` when already assigned.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Full`] beyond the assignment limit.
    pub fn assign(
        &mut self,
        agent_id: &str,
        workspace_id: &str,
        at: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        if self.assigned(agent_id) {
            return Ok(false);
        }
        if self.assignments.len() >= ASSIGNMENT_LIMIT {
            return Err(TicketError::Full);
        }
        self.assignments.push(Assignment {
            agent_id: agent_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            assigned_at: at,
        });
        self.updated_at = at;
        Ok(true)
    }

    /// Whether `agent_id` is assigned to this ticket.
    #[must_use]
    pub fn assigned(&self, agent_id: &str) -> bool {
        self.assignments
            .iter()
            .any(|assignment| assignment.agent_id == agent_id)
    }

    /// Record the system report for `notification` about one of this ticket's Agents, admitted
    /// at `at`, and apply its automatic transition: a permission request makes an open ticket
    /// wait, and the same Agent finishing or failing ends that wait. Closed tickets only record
    /// it. Notifications that are not about an Agent's turn change nothing.
    pub fn record_notification(&mut self, notification: &Notification, at: DateTime<Utc>) {
        let kind = match notification.kind {
            NotificationKind::AgentFinished => ReportKind::AgentFinished,
            NotificationKind::AgentFailed => ReportKind::AgentFailed,
            NotificationKind::AgentPermission => ReportKind::AgentPermission,
            NotificationKind::ScheduleSucceeded
            | NotificationKind::ScheduleFailed
            | NotificationKind::TerminalAttention
            | NotificationKind::TicketBlocked
            | NotificationKind::TicketDone
            | NotificationKind::TicketFailed
            | NotificationKind::PullMerged
            | NotificationKind::PullClosed
            | NotificationKind::PullChecksPassed
            | NotificationKind::PullActivity => return,
        };
        let agent = notification.agent_id.as_deref();
        let status = match (kind, agent) {
            (ReportKind::AgentPermission, Some(agent)) if self.status == TicketStatus::Open => {
                Some(self.wait(WaitReason::Permission, agent, at))
            }
            (ReportKind::AgentFinished | ReportKind::AgentFailed, Some(agent))
                if self.waits_on(agent, Some(WaitReason::Permission)) =>
            {
                Some(self.resume())
            }
            _ => None,
        };
        if let (ReportKind::AgentFinished | ReportKind::AgentFailed, Some(agent)) = (kind, agent) {
            // Any turn end after the send ends the turn the input started.
            self.input_turns.retain(|turn| {
                turn.agent_id != agent || notification.at < turn.sent.unwrap_or(turn.at)
            });
        }
        self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::System,
            kind,
            agent_id: agent.map(str::to_owned),
            text: clip(
                notification.summary.as_deref().unwrap_or_default(),
                TEXT_LIMIT,
            )
            .to_owned(),
            status,
        });
    }

    /// Record what `agent_id`, a worker the ticket's daemon-written label names, reports at
    /// `at` and apply its automatic transition; the Agent need not hold an assignment. `blocked`
    /// makes the ticket wait on the reporter unless another worker's block already holds it,
    /// replacing a permission wait; any other report lifts the reporter's own block or wait and
    /// hands the wait to the next blocked worker, if any. A `blocked` report may carry the one
    /// `question` the worker asks the user, bounded; the wait shows it (ADR-287). Returns the
    /// report's sequence.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Closed`] for a closed ticket and [`TicketError::Invalid`] for a
    /// question on a report other than `blocked`.
    pub fn report(
        &mut self,
        agent_id: &str,
        kind: WorkerReportKind,
        text: &str,
        question: Option<&str>,
        at: DateTime<Utc>,
    ) -> Result<u64, TicketError> {
        if self.status.is_closed() {
            return Err(TicketError::Closed);
        }
        if question.is_some() && kind != WorkerReportKind::Blocked {
            return Err(TicketError::Invalid);
        }
        let question = question
            .map(str::trim)
            .filter(|question| !question.is_empty())
            .map(|question| clip(question, QUESTION_LIMIT).to_owned());
        let status = match kind {
            WorkerReportKind::Blocked => self.block(agent_id, question, at),
            WorkerReportKind::Progress | WorkerReportKind::Done | WorkerReportKind::Failed => {
                self.unblock(agent_id)
            }
        };
        let sequence = self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::Agent,
            kind: kind.report_kind(),
            agent_id: Some(agent_id.to_owned()),
            text: clip(text, TEXT_LIMIT).to_owned(),
            status,
        });
        if kind == WorkerReportKind::Done && self.resumed() {
            self.end_followed(
                "Done again: the worker finished the activity that resumed this ticket",
                at,
            );
        }
        Ok(sequence)
    }

    /// Whether the end of an Agent's turn, `notification`, should not wake the Bot: a finished
    /// turn that a delivered pull request input started, after the input was sent and before
    /// the provider accepted it, in which the worker reported nothing that wakes the Bot
    /// (ADR-287); or a turn end that would only repeat a wake-up the Bot already had for the
    /// same event: that Agent's latest worker report recorded since its previous turn end on
    /// this ticket was made in the turn's window and is an outcome (`blocked`, `done` or
    /// `failed`; only `failed` for a failed turn). The window runs from the later of the
    /// turn's start, as the daemon dated it, and that previous turn end, to the turn's end.
    /// Permission requests and turns whose start is unknown are never spared.
    #[must_use]
    pub fn spares_turn_end(&self, notification: &Notification) -> bool {
        let failed = match notification.kind {
            NotificationKind::AgentFinished => false,
            NotificationKind::AgentFailed => true,
            NotificationKind::AgentPermission
            | NotificationKind::ScheduleSucceeded
            | NotificationKind::ScheduleFailed
            | NotificationKind::TerminalAttention
            | NotificationKind::TicketBlocked
            | NotificationKind::TicketDone
            | NotificationKind::TicketFailed
            | NotificationKind::PullMerged
            | NotificationKind::PullClosed
            | NotificationKind::PullChecksPassed
            | NotificationKind::PullActivity => return false,
        };
        let (Some(agent), Some(started)) = (
            notification.agent_id.as_deref(),
            notification.turn_started_at,
        ) else {
            return false;
        };
        // Reports are appended in admission order, so those after the Agent's latest recorded
        // turn end belong to a later turn.
        let mut turn_reports = self
            .reports
            .iter()
            .rev()
            .filter(|report| report.agent_id.as_deref() == Some(agent))
            .take_while(|report| {
                !(report.source == ReportSource::System
                    && matches!(
                        report.kind,
                        ReportKind::AgentFinished | ReportKind::AgentFailed
                    ))
            })
            .filter(|report| report.source == ReportSource::Agent);
        // A finished turn a delivered pull request input started, in which the worker reported
        // nothing that wakes the Bot, ends without waking it: the input was for the worker,
        // and outcomes reach the Bot as their own notifications. A failed turn still wakes it.
        if !failed
            && self.input_turns.iter().any(|turn| {
                turn.agent_id == agent
                    && turn.sent.is_some_and(|sent| sent <= started)
                    && started <= turn.at
                    && turn.at <= notification.at
            })
            && !turn_reports.clone().any(|report| {
                started <= report.at
                    && matches!(
                        report.kind,
                        ReportKind::Blocked | ReportKind::Done | ReportKind::Failed
                    )
            })
        {
            return true;
        }
        let latest = turn_reports.next();
        latest.is_some_and(|report| {
            started <= report.at
                && report.at <= notification.at
                && match report.kind {
                    ReportKind::Failed => true,
                    ReportKind::Blocked | ReportKind::Done => !failed,
                    ReportKind::Progress
                    | ReportKind::AgentFinished
                    | ReportKind::AgentFailed
                    | ReportKind::AgentPermission
                    | ReportKind::Note
                    | ReportKind::Status
                    | ReportKind::Pull
                    | ReportKind::Answer => false,
                }
        })
    }

    /// Close this ticket on Ait's behalf at `at` because every Agent working on it is gone, with
    /// `reason` as the outcome and the text of a system report. A ticket that follows an open
    /// pull request closes as `done`, so it keeps following it until it ends (ADR-287). Any
    /// other open ticket closes as `done` only when every assigned Agent, and every other
    /// worker that reported, last reported `done` since the ticket was last reopened;
    /// otherwise, and always for a waiting ticket, which nobody can answer any more, as
    /// `cancelled`. Closed tickets and tickets without assignments are left alone (`None`);
    /// otherwise returns the new status.
    pub fn close_finished(&mut self, reason: &str, at: DateTime<Utc>) -> Option<TicketStatus> {
        if self.status.is_closed() || self.assignments.is_empty() {
            return None;
        }
        let target = if self.follows_open_pull()
            || (self.status == TicketStatus::Open && self.finished_since_reopened())
        {
            TicketStatus::Done
        } else {
            TicketStatus::Cancelled
        };
        self.change_for_ait(target, reason, at);
        Some(target)
    }

    /// Whether every assigned Agent and every other reporting worker last reported `done`
    /// among the reports kept since the ticket was last reopened.
    fn finished_since_reopened(&self) -> bool {
        let mut closed = false;
        let mut current = 0;
        for (index, report) in self.reports.iter().enumerate() {
            let Some(status) = report.status else {
                continue;
            };
            if closed && status == TicketStatus::Open {
                current = index;
            }
            closed = status.is_closed();
        }
        let reports = &self.reports[current..];
        let latest = |agent: &str| {
            reports.iter().rev().find(|report| {
                report.source == ReportSource::Agent && report.agent_id.as_deref() == Some(agent)
            })
        };
        let done =
            |report: Option<&Report>| report.is_some_and(|report| report.kind == ReportKind::Done);
        self.assignments
            .iter()
            .all(|assignment| done(latest(&assignment.agent_id)))
            && reports
                .iter()
                .filter(|report| report.source == ReportSource::Agent)
                .filter_map(|report| report.agent_id.as_deref())
                .all(|agent| done(latest(agent)))
    }

    /// Whether this ticket, at `at`, looks like what a crash between opening it and creating
    /// its first Agent leaves behind: unclosed, without assignments, without any report about
    /// an Agent, and opened at least [`ORPHAN_GRACE`] ago. Bot notes and status changes are no
    /// sign of an Agent. An Agent can carry the ticket's label before it is assigned, so the
    /// caller must also know that no Agent, archived or not, carries it.
    #[must_use]
    pub fn orphaned(&self, at: DateTime<Utc>) -> bool {
        !self.status.is_closed()
            && self.assignments.is_empty()
            && self.reports.iter().all(|report| report.agent_id.is_none())
            && self.created_at + ORPHAN_GRACE <= at
    }

    /// Reopen this closed ticket on Ait's behalf at `at`, recording `reason` as a system
    /// report; `false` when it is not closed.
    pub fn reopen(&mut self, reason: &str, at: DateTime<Utc>) -> bool {
        if !self.status.is_closed() {
            return false;
        }
        self.change_for_ait(TicketStatus::Open, reason, at);
        true
    }

    /// Apply `update` from `source` (the Bot or the user) at `at`; `false` when it changes
    /// nothing, such as closing a ticket again with the same status.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Invalid`] for an empty update, a `waiting` target, a blank title
    /// or an outcome without a closing status, and [`TicketError::Conflict`] for closing a
    /// closed ticket with another status, except cancelling a `done` or `failed` ticket, which
    /// stops following its pull requests (ADR-287). A refused update changes nothing.
    pub fn update(
        &mut self,
        source: ReportSource,
        update: &TicketUpdate,
        at: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        let empty = update.status.is_none()
            && update.title.is_none()
            && update.note.is_none()
            && update.answer.is_none()
            && update.pull.is_none();
        let closing = update.status.is_some_and(TicketStatus::is_closed);
        if empty
            || update.status == Some(TicketStatus::Waiting)
            || (update.outcome.is_some() && !closing)
            || (update.answer.is_some() && update.status.is_some())
            || update
                .answer
                .as_ref()
                .is_some_and(|answer| answer.text.trim().is_empty())
        {
            return Err(TicketError::Invalid);
        }
        if update
            .answer
            .as_ref()
            .is_some_and(|answer| !self.waits_for_answer_from(&answer.agent_id))
        {
            return Err(TicketError::Conflict);
        }
        if let Some(snapshot) = &update.pull {
            self.check_pull(snapshot.number)?;
        }
        let title = update.title.as_deref().map(valid_title).transpose()?;
        if let Some(target) = update.status
            && target.is_closed()
            && self.status.is_closed()
            && self.status != target
            && target != TicketStatus::Cancelled
        {
            return Err(TicketError::Conflict);
        }
        let mut changed = false;
        if let Some(title) = title
            && title != self.title
        {
            title.clone_into(&mut self.title);
            self.updated_at = at;
            changed = true;
        }
        let outcome = update.outcome.as_deref().map(|text| clip(text, TEXT_LIMIT));
        if let Some(target) = update.status {
            // Closing again with the same status keeps the closing time but takes a new
            // outcome, such as the Bot's summary after the user closed the ticket.
            let moved = target != self.status;
            let concluded = !moved && outcome.is_some() && self.outcome.as_deref() != outcome;
            if moved && self.status.is_closed() && outcome.is_none() {
                // Cancelling a closed ticket to stop following keeps its conclusion.
                let kept = self.outcome.take();
                self.move_to(target, kept.as_deref(), at);
            } else if moved {
                self.move_to(target, outcome, at);
            } else if concluded {
                self.outcome = outcome.map(str::to_owned);
            }
            if moved || concluded {
                self.record(Report {
                    sequence: 0,
                    at,
                    source,
                    kind: ReportKind::Status,
                    agent_id: None,
                    text: outcome.unwrap_or_default().to_owned(),
                    status: Some(target),
                });
                changed = true;
            }
        }
        if let Some(note) = &update.note {
            self.record(Report {
                sequence: 0,
                at,
                source,
                kind: ReportKind::Note,
                agent_id: None,
                text: clip(note, TEXT_LIMIT).to_owned(),
                status: None,
            });
            changed = true;
        }
        if let Some(snapshot) = &update.pull {
            let bound_by = match source {
                ReportSource::Agent => BoundBy::Agent,
                ReportSource::Bot | ReportSource::User | ReportSource::System => BoundBy::Bot,
            };
            changed |= self.bind_pull(snapshot, bound_by, None, at)?;
        }
        if let Some(answer) = &update.answer {
            self.answer(source, &answer.agent_id, &answer.text, at)?;
            changed = true;
        }
        Ok(changed)
    }

    /// Whether the ticket waits on blocked worker `agent_id`, which an answer can unblock.
    #[must_use]
    pub fn waits_for_answer_from(&self, agent_id: &str) -> bool {
        !self.status.is_closed() && self.waits_on(agent_id, Some(WaitReason::Blocked))
    }

    /// Record at `at` the answer `text` that `source` (the user, or the Bot for them) gives
    /// `agent_id`, the blocked worker the ticket waits on, lift that worker's block as its own
    /// report would, and queue the answer for delivery to it. Returns the receiving Agent.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Closed`] for a closed ticket, [`TicketError::Conflict`] when the
    /// ticket does not wait on `agent_id` as a blocked worker, for instance because the wait
    /// passed to another worker since the question was shown, and [`TicketError::Invalid`] for
    /// a blank answer.
    pub fn answer(
        &mut self,
        source: ReportSource,
        agent_id: &str,
        text: &str,
        at: DateTime<Utc>,
    ) -> Result<String, TicketError> {
        if self.status.is_closed() {
            return Err(TicketError::Closed);
        }
        let text = text.trim();
        if text.is_empty() {
            return Err(TicketError::Invalid);
        }
        let waiting = self
            .waiting
            .clone()
            .filter(|waiting| waiting.reason == WaitReason::Blocked && waiting.agent_id == agent_id)
            .ok_or(TicketError::Conflict)?;
        let text = clip(text, TEXT_LIMIT);
        let status = self.unblock(&waiting.agent_id);
        let sequence = self.record(Report {
            sequence: 0,
            at,
            source,
            kind: ReportKind::Answer,
            agent_id: Some(waiting.agent_id.clone()),
            text: text.to_owned(),
            status,
        });
        let who = if source == ReportSource::Bot {
            "The Workspace Bot answered on the user's behalf"
        } else {
            "The user answered"
        };
        let question = waiting
            .question
            .as_deref()
            .map(|question| format!("Your question: {question}\n"))
            .unwrap_or_default();
        let body = format!(
            "{who} after you reported that you are blocked on ticket \"{}\".\n{question}\
             Answer: {text}\nContinue your work with this answer.",
            self.title
        );
        let ticket = self.id.to_string();
        self.pending.push(Delivery::new(
            format!("answer:{ticket}:{sequence}"),
            waiting.agent_id.clone(),
            input_envelope::ticket_answer(&ticket, &body),
        ));
        Ok(waiting.agent_id)
    }

    /// Follow the pull request `snapshot` shows, bound by `bound_by` (worker `agent_id`) at
    /// `at`, recording it in the history; binding a followed number again only refreshes it.
    /// Returns whether a new pull request was bound.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Closed`] for a closed ticket and [`TicketError::Full`] beyond
    /// [`PULL_LIMIT`].
    pub fn bind_pull(
        &mut self,
        snapshot: &PullSnapshot,
        bound_by: BoundBy,
        agent_id: Option<&str>,
        at: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        self.check_pull(snapshot.number)?;
        if let Some(bound) = self
            .pulls
            .iter_mut()
            .find(|binding| binding.number == snapshot.number)
        {
            let refreshed = PullBinding::new(snapshot, bound.bound_by, None, at);
            bound.url = refreshed.url;
            bound.title = refreshed.title;
            return Ok(false);
        }
        let binding = PullBinding::new(snapshot, bound_by, agent_id, at);
        let source = match bound_by {
            BoundBy::Agent => ReportSource::Agent,
            BoundBy::Bot => ReportSource::Bot,
        };
        self.record(Report {
            sequence: 0,
            at,
            source,
            kind: ReportKind::Pull,
            agent_id: agent_id.map(str::to_owned),
            text: clip(
                &format!(
                    "Following pull request #{}: {} at {}",
                    binding.number,
                    data(&binding.title),
                    data(&binding.url)
                ),
                TEXT_LIMIT,
            )
            .to_owned(),
            status: None,
        });
        self.pulls.push(binding);
        Ok(true)
    }

    /// Whether pull request `number` may be bound: the ticket is not closed and follows it
    /// already or has room.
    fn check_pull(&self, number: u64) -> Result<(), TicketError> {
        if self.status.is_closed() {
            return Err(TicketError::Closed);
        }
        if self.pulls.len() >= PULL_LIMIT && !self.pulls.iter().any(|pull| pull.number == number) {
            return Err(TicketError::Full);
        }
        Ok(())
    }

    /// Whether the follower reads this ticket's pull requests at `now`: it is not closed; it
    /// closed as `done` or `failed` while one of them is still open, so the follow continues
    /// until it ends; or its pull requests ended less than [`FOLLOW_AFTER`] ago. A cancelled
    /// ticket's work is abandoned, so it follows nothing.
    #[must_use]
    pub fn follows_pulls(&self, now: DateTime<Utc>) -> bool {
        if self.pulls.is_empty() {
            return false;
        }
        match self.status {
            TicketStatus::Open | TicketStatus::Waiting => true,
            TicketStatus::Cancelled => false,
            TicketStatus::Done | TicketStatus::Failed => {
                self.follow_until.is_some_and(|until| now < until) || self.follows_open_pull()
            }
        }
    }

    /// Whether every pull request the ticket follows ended, so it is read less often.
    #[must_use]
    pub fn pulls_ended(&self) -> bool {
        !self.pulls.is_empty() && self.pulls.iter().all(|pull| pull.state.is_ended())
    }

    /// Whether one of the ticket's pull requests is still open.
    #[must_use]
    pub fn follows_open_pull(&self) -> bool {
        self.pulls.iter().any(|pull| !pull.state.is_ended())
    }

    /// Whether the ticket is open again after its pull requests ended it: they all ended and
    /// it still has a follow window.
    fn resumed(&self) -> bool {
        !self.status.is_closed() && self.pulls_ended() && self.follow_until.is_some()
    }

    /// Close the ticket as `done` for Ait at `at` with `reason`, keeping its follow window, or
    /// opening one when its pull requests just ended.
    fn end_followed(&mut self, reason: &str, at: DateTime<Utc>) {
        let until = self.follow_until.unwrap_or(at + FOLLOW_AFTER);
        self.change_for_ait(TicketStatus::Done, reason, at);
        self.follow_until = Some(until);
    }

    /// Close a ticket that resumed after its pull requests ended once its follow window passed
    /// at `now`; `true` when it closed.
    pub fn expire_follow(&mut self, now: DateTime<Utc>) -> bool {
        if !self.resumed() || self.follow_until.is_some_and(|until| now < until) {
            return false;
        }
        self.end_followed(
            "Done again: nothing new arrived on its ended pull requests within the follow window",
            now,
        );
        true
    }

    /// Record at `at` that reading pull request `number` failed with `error`, `missing` when
    /// the forge found no such pull request. Once it was missing for a number of reads in a
    /// row, such as after the repository was deleted or moved, the ticket stops following it,
    /// records why, and frees its slot; a ticket that is not closed tells the Bot (ADR-287).
    /// Returns `None` when the ticket does not follow `number`.
    pub fn pull_failed(
        &mut self,
        number: u64,
        error: &str,
        missing: bool,
        at: DateTime<Utc>,
    ) -> Option<PullFailure> {
        let index = self.pulls.iter().position(|pull| pull.number == number)?;
        let dropped = self.pulls[index].failed(error, missing);
        let notices = if dropped {
            self.stop_following(index, at)
        } else {
            Vec::new()
        };
        Some(PullFailure { dropped, notices })
    }

    /// Stop following pull request `index`, which the forge kept reporting missing, recording
    /// why at `at`; returns the notice that tells the Bot, only for a ticket that is not closed.
    fn stop_following(&mut self, index: usize, at: DateTime<Utc>) -> Vec<PullNotice> {
        let binding = self.pulls.remove(index);
        let reason = format!(
            "Stopped following pull request #{}: the forge reported it missing on {} reads in \
             a row, so its repository was probably deleted or moved, or it is no longer visible \
             to this host's forge account",
            binding.number, binding.missing
        );
        let sequence = self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::System,
            kind: ReportKind::Pull,
            agent_id: binding.agent_id.clone(),
            text: reason.clone(),
            status: None,
        });
        if self.status.is_closed() {
            return Vec::new();
        }
        vec![PullNotice {
            id: format!("pull:{}:{sequence}:missing", self.id),
            kind: NotificationKind::PullActivity,
            agent_id: binding.agent_id,
            title: format!(
                "PR #{} can no longer be read: {}",
                binding.number,
                data(&binding.title)
            ),
            summary: Some(reason),
        }]
    }

    /// Apply one read of followed pull request `number` (ADR-287): route what changed to the
    /// ticket's live workers as pending inputs, or to the Bot as notices; entries by people who
    /// cannot write to the repository only reach the Bot. Once every pull request it follows
    /// ended and one merged, end the ticket as `done` (a ticket already closed keeps its
    /// status) and keep following for [`FOLLOW_AFTER`]. A closed ticket that still follows
    /// gets worker input only by reopening, within the open-ticket limit and when a worker is
    /// live; otherwise that input goes to the Bot. The forge account's own entries reopen a
    /// closed ticket only while one of its pull requests is open: once they all ended, they
    /// are the owner's remarks on finished work. Returns `None` when the ticket does not follow
    /// `number`.
    pub fn observe_pull(
        &mut self,
        number: u64,
        observation: PullObservation<'_>,
    ) -> Option<Followed> {
        let at = observation.at;
        if !self.follows_pulls(at) {
            return None;
        }
        let index = self.pulls.iter().position(|pull| pull.number == number)?;
        let ended = self.pulls_ended();
        let (mut events, changed) = self.pulls[index].observe(
            observation.snapshot,
            observation.authored,
            observation.lookups,
            at,
        );
        let mut followed = Followed {
            changed,
            ..Followed::default()
        };
        let mut live = observation.live;
        if self.status.is_closed() {
            // Once every pull request ended, the forge account's own entries do not reopen
            // the ticket; while one is open they are the owner's review like anyone else's.
            if ended {
                events.retain(|event| !matches!(event, PullEvent::Item(item) if item.own));
            }
            let for_worker = events.iter().any(PullEvent::for_worker);
            if for_worker && observation.reopen && !self.receivers(index, live).is_empty() {
                let until = self.follow_until;
                let reason = format!(
                    "Reopened automatically: new review activity on pull request #{number}"
                );
                self.change_for_ait(TicketStatus::Open, &reason, at);
                self.follow_until = until.map(|until| until.max(at + FOLLOW_AFTER));
            } else {
                live = &[];
            }
        }
        if events.is_empty() {
            return Some(followed);
        }
        self.route(index, &events, live, &mut followed, at);
        // Only the read that ends the last pull request ends the ticket, so a comment that
        // resumed it does not end it again.
        let ends = events
            .iter()
            .any(|event| matches!(event, PullEvent::Merged | PullEvent::Closed));
        if ends
            && self.pulls_ended()
            && self
                .pulls
                .iter()
                .any(|pull| pull.state == PullState::Merged)
        {
            if self.status.is_closed() {
                self.follow_until = Some(at + FOLLOW_AFTER);
            } else {
                let merged: Vec<String> = self
                    .pulls
                    .iter()
                    .filter(|pull| pull.state == PullState::Merged)
                    .map(|pull| format!("#{}", pull.number))
                    .collect();
                self.follow_until = None;
                self.end_followed(&format!("Pull request {} merged", merged.join(", ")), at);
                followed.closed = true;
            }
        }
        Some(followed)
    }

    /// The workers that receive pull request `index`'s worker events: its binding worker if it
    /// is in `live`, else every worker in `live`.
    fn receivers(&self, index: usize, live: &[String]) -> Vec<String> {
        match &self.pulls[index].agent_id {
            Some(agent) if live.contains(agent) => vec![agent.clone()],
            _ => live.to_vec(),
        }
    }

    /// Route `events` of pull request `index`: inputs for the binding worker if it is live,
    /// else for every live worker; worker events without a live worker or room in the pending
    /// inputs, and entries by people who cannot write to the repository, become one notice
    /// each; outcomes become their own notices.
    fn route(
        &mut self,
        index: usize,
        events: &[PullEvent],
        live: &[String],
        followed: &mut Followed,
        at: DateTime<Utc>,
    ) {
        let for_worker: Vec<&PullEvent> =
            events.iter().filter(|event| event.for_worker()).collect();
        let outside: Vec<&PullEvent> = events
            .iter()
            .filter(|event| matches!(event, PullEvent::Outside(_)))
            .collect();
        let receivers = self.receivers(index, live);
        let delivered = !for_worker.is_empty()
            && !receivers.is_empty()
            && self.pending.len() + receivers.len() <= PENDING_LIMIT;
        let binding = &self.pulls[index];
        let summary = summarize(binding, events, delivered.then_some(receivers.as_slice()));
        let sequence = self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::System,
            kind: ReportKind::Pull,
            agent_id: binding.agent_id.clone(),
            text: clip(&summary, TEXT_LIMIT).to_owned(),
            status: None,
        });
        let binding = &self.pulls[index];
        let ticket = self.id.to_string();
        let notice =
            |key: &str, kind: NotificationKind, title: String, summary: String| PullNotice {
                id: format!("pull:{ticket}:{sequence}:{key}"),
                kind,
                agent_id: binding.agent_id.clone(),
                title,
                summary: Some(summary),
            };
        let mut notices = Vec::new();
        let mut inputs = Vec::new();
        if delivered {
            let text = input_envelope::ticket_pull(
                &ticket,
                binding.number,
                &worker_text(binding, &for_worker),
            );
            inputs.extend(receivers.into_iter().map(|agent| {
                Delivery::new(
                    format!("{PULL_INPUT}{ticket}:{sequence}:{agent}"),
                    agent,
                    text.clone(),
                )
            }));
        } else if let Some(first) = for_worker.first() {
            notices.push(notice(
                "activity",
                NotificationKind::PullActivity,
                bot_title(binding, first),
                worker_text(binding, &for_worker),
            ));
        }
        if let Some(first) = outside.first() {
            notices.push(notice(
                "outside",
                NotificationKind::PullActivity,
                bot_title(binding, first),
                outside_text(binding, &outside),
            ));
        }
        for event in events {
            let (kind, key) = match event {
                PullEvent::Merged => (NotificationKind::PullMerged, "merged"),
                PullEvent::Closed => (NotificationKind::PullClosed, "closed"),
                PullEvent::ChecksPassed => (NotificationKind::PullChecksPassed, "checks"),
                PullEvent::Item(_) | PullEvent::Outside(_) | PullEvent::CheckFailed(_) => continue,
            };
            notices.push(notice(
                key,
                kind,
                bot_title(binding, event),
                format!("{} at {}", data(&binding.title), data(&binding.url)),
            ));
        }
        self.pending.extend(inputs);
        followed.notices.extend(notices);
        followed.changed = true;
    }

    /// Forget the pending inputs whose ids are in `ids`, which were sent from `sent` on and
    /// which the provider accepted by `at`, each as a new turn of its idle Agent, and remember
    /// the turns that pull request inputs started (ADR-287). Answers start no remembered turn,
    /// and neither does an input whose Agent's turn end was recorded since `sent`: that turn
    /// is over. Returns `false` when none of them was pending.
    pub fn delivered(&mut self, ids: &[String], sent: DateTime<Utc>, at: DateTime<Utc>) -> bool {
        let before = self.pending.len();
        let mut agents = Vec::new();
        self.pending.retain(|delivery| {
            let done = ids.contains(&delivery.id);
            if done && delivery.id.starts_with(PULL_INPUT) {
                agents.push(delivery.agent_id.clone());
            }
            !done
        });
        for agent in agents {
            self.input_turns.retain(|turn| turn.agent_id != agent);
            let ended = self.reports.iter().any(|report| {
                report.source == ReportSource::System
                    && matches!(
                        report.kind,
                        ReportKind::AgentFinished | ReportKind::AgentFailed
                    )
                    && report.agent_id.as_deref() == Some(agent.as_str())
                    && report.at >= sent
            });
            if !ended && self.input_turns.len() < ASSIGNMENT_LIMIT {
                self.input_turns.push(InputTurn {
                    agent_id: agent,
                    sent: Some(sent),
                    at,
                });
            }
        }
        self.pending.len() != before
    }

    /// Hand the pending inputs for Agents not in `live` to the Bot as notices, recording at
    /// `at` that they could not be delivered, and drop them.
    pub fn reroute_pending(&mut self, live: &[String], at: DateTime<Utc>) -> Vec<PullNotice> {
        let (stranded, kept): (Vec<Delivery>, Vec<Delivery>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|delivery| !live.contains(&delivery.agent_id));
        self.pending = kept;
        stranded
            .into_iter()
            .map(|delivery| {
                let reason = format!(
                    "Agent {} is archived, so an input for it went to the Bot instead",
                    delivery.agent_id
                );
                let title = format!("Undelivered input for an archived Agent: {}", self.title);
                self.strand(delivery, reason, title, at)
            })
            .collect()
    }

    /// Count a failed attempt at `at` against each pending input in `ids`. A failure more
    /// than [`DELIVERY_FAILURE_WINDOW`] after the one before starts a new run; an input whose
    /// live Agent failed to take it [`DELIVERY_FAILURE_LIMIT`] times in one run that has gone
    /// on for the window goes to the Bot as a notice instead, with the reason recorded
    /// (ADR-287). Returns `None` when none of them was pending, else the notices.
    pub fn delivery_failed(
        &mut self,
        ids: &[String],
        at: DateTime<Utc>,
    ) -> Option<Vec<PullNotice>> {
        let mut counted = false;
        let mut stranded = Vec::new();
        self.pending.retain_mut(|delivery| {
            if !ids.contains(&delivery.id) {
                return true;
            }
            counted = true;
            if delivery
                .failed_at
                .is_none_or(|last| at - last > DELIVERY_FAILURE_WINDOW)
            {
                delivery.failures = 0;
                delivery.failing_since = Some(at);
            }
            delivery.failures = delivery.failures.saturating_add(1);
            delivery.failed_at = Some(at);
            let lasted = delivery
                .failing_since
                .is_some_and(|since| at - since >= DELIVERY_FAILURE_WINDOW);
            if delivery.failures < DELIVERY_FAILURE_LIMIT || !lasted {
                return true;
            }
            stranded.push(delivery.clone());
            false
        });
        counted.then(|| {
            stranded
                .into_iter()
                .map(|delivery| {
                    let reason = format!(
                        "Agent {} failed to take an input {} times for {} minutes, so it went \
                         to the Bot instead",
                        delivery.agent_id,
                        delivery.failures,
                        DELIVERY_FAILURE_WINDOW.num_minutes()
                    );
                    let title =
                        format!("Undelivered input after repeated failures: {}", self.title);
                    self.strand(delivery, reason, title, at)
                })
                .collect()
        })
    }

    /// Start a new run of failures for each pending input in `ids`, whose Agent answered that
    /// it is busy: it is reachable and takes the input once idle. Returns whether one had
    /// failures to forget.
    pub fn delivery_busy(&mut self, ids: &[String]) -> bool {
        let mut reset = false;
        for delivery in &mut self.pending {
            if ids.contains(&delivery.id) && delivery.failed_at.is_some() {
                delivery.failures = 0;
                delivery.failing_since = None;
                delivery.failed_at = None;
                reset = true;
            }
        }
        reset
    }

    /// Record at `at` why `delivery` was not delivered, `reason`, and return the notice that
    /// hands its text to the Bot under `title`.
    fn strand(
        &mut self,
        delivery: Delivery,
        reason: String,
        title: String,
        at: DateTime<Utc>,
    ) -> PullNotice {
        let sequence = self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::System,
            kind: ReportKind::Pull,
            agent_id: Some(delivery.agent_id.clone()),
            text: reason,
            status: None,
        });
        PullNotice {
            id: format!("pull:{}:{sequence}:stranded", self.id),
            kind: NotificationKind::PullActivity,
            agent_id: Some(delivery.agent_id),
            title,
            summary: input_envelope::parse(&delivery.text)
                .map(|envelope| envelope.text.into_owned()),
        }
    }

    /// Move to `target` at `at` for Ait, recording `reason` as a system status report and, when
    /// closing, as the outcome.
    fn change_for_ait(&mut self, target: TicketStatus, reason: &str, at: DateTime<Utc>) {
        let reason = clip(reason, TEXT_LIMIT);
        self.move_to(target, Some(reason), at);
        self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::System,
            kind: ReportKind::Status,
            agent_id: None,
            text: reason.to_owned(),
            status: Some(target),
        });
    }

    /// Move to `target` at `at` by an explicit update: close with `outcome`, reopen, or end a
    /// wait; every worker's block is lifted.
    fn move_to(&mut self, target: TicketStatus, outcome: Option<&str>, at: DateTime<Utc>) {
        self.status = target;
        self.waiting = None;
        self.blocked.clear();
        self.follow_until = None;
        if target.is_closed() {
            self.closed_at = Some(at);
            self.outcome = outcome.map(str::to_owned);
        } else {
            self.closed_at = None;
            self.outcome = None;
        }
    }

    /// Whether the ticket waits on `agent_id`, for `reason` when given.
    fn waits_on(&self, agent_id: &str, reason: Option<WaitReason>) -> bool {
        self.waiting.as_ref().is_some_and(|waiting| {
            waiting.agent_id == agent_id && reason.is_none_or(|reason| waiting.reason == reason)
        })
    }

    /// Remember that `agent_id` is blocked since `at`, asking `question`, and make the ticket
    /// wait on it unless another worker's block already does; a worker that reports blocked
    /// again keeps its place and takes its new question. Returns the status when the wait
    /// changed.
    fn block(
        &mut self,
        agent_id: &str,
        question: Option<String>,
        at: DateTime<Utc>,
    ) -> Option<TicketStatus> {
        if let Some(block) = self
            .blocked
            .iter_mut()
            .find(|block| block.agent_id == agent_id)
        {
            block.question.clone_from(&question);
        } else if self.blocked.len() < ASSIGNMENT_LIMIT {
            self.blocked.push(Waiting {
                reason: WaitReason::Blocked,
                agent_id: agent_id.to_owned(),
                since: at,
                question: question.clone(),
            });
        }
        if let Some(waiting) = self
            .waiting
            .as_mut()
            .filter(|waiting| waiting.reason == WaitReason::Blocked && waiting.agent_id == agent_id)
        {
            waiting.question = question;
            return None;
        }
        let held = self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.reason == WaitReason::Blocked);
        (!held).then(|| {
            let status = self.wait(WaitReason::Blocked, agent_id, at);
            if let Some(waiting) = self.waiting.as_mut() {
                waiting.question = question;
            }
            status
        })
    }

    /// Lift `agent_id`'s block; when the ticket waited on it, wait on the next blocked worker
    /// or reopen. Returns the status when the wait changed.
    fn unblock(&mut self, agent_id: &str) -> Option<TicketStatus> {
        self.blocked.retain(|block| block.agent_id != agent_id);
        if !self.waits_on(agent_id, None) {
            return None;
        }
        Some(match self.blocked.first() {
            Some(next) => {
                self.waiting = Some(next.clone());
                self.status
            }
            None => self.resume(),
        })
    }

    fn wait(&mut self, reason: WaitReason, agent_id: &str, at: DateTime<Utc>) -> TicketStatus {
        self.status = TicketStatus::Waiting;
        self.waiting = Some(Waiting {
            reason,
            agent_id: agent_id.to_owned(),
            since: at,
            question: None,
        });
        self.status
    }

    fn resume(&mut self) -> TicketStatus {
        self.status = TicketStatus::Open;
        self.waiting = None;
        self.status
    }

    /// Append `report` with the next sequence, forgetting the oldest beyond the report limit,
    /// and return its sequence.
    fn record(&mut self, mut report: Report) -> u64 {
        let sequence = self.next_report;
        report.sequence = sequence;
        self.updated_at = report.at;
        self.next_report = sequence.saturating_add(1);
        self.reports.push(report);
        let excess = self.reports.len().saturating_sub(REPORT_LIMIT);
        self.reports.drain(..excess);
        sequence
    }
}

/// The history line for `events` of pull request `binding`, naming the workers that receive the
/// worker events, if any.
fn summarize(binding: &PullBinding, events: &[PullEvent], receivers: Option<&[String]>) -> String {
    let mut parts = Vec::new();
    let items = events
        .iter()
        .filter(|event| matches!(event, PullEvent::Item(_)))
        .count();
    if items > 0 {
        parts.push(format!("{items} new review entries"));
    }
    let outside = events
        .iter()
        .filter(|event| matches!(event, PullEvent::Outside(_)))
        .count();
    if outside > 0 {
        parts.push(format!(
            "{outside} entries by people who cannot write to the repository, told to the Bot only"
        ));
    }
    for event in events {
        match event {
            PullEvent::CheckFailed(check) => {
                parts.push(format!("check {} failed", data(&check.name)));
            }
            PullEvent::ChecksPassed => parts.push("checks pass again".to_owned()),
            PullEvent::Merged => parts.push("merged".to_owned()),
            PullEvent::Closed => parts.push("closed without merging".to_owned()),
            PullEvent::Item(_) | PullEvent::Outside(_) => {}
        }
    }
    let routed = match receivers {
        Some(agents) => format!("; delivered to {}", agents.join(", ")),
        None if events.iter().any(PullEvent::for_worker) => "; no worker could receive it, so \
            the Bot was told"
            .to_owned(),
        None => String::new(),
    };
    format!(
        "Pull request #{}: {}{routed}",
        binding.number,
        parts.join(", ")
    )
}

/// `title` trimmed and bounded.
///
/// # Errors
///
/// Returns [`TicketError::Invalid`] when nothing is left after trimming.
fn valid_title(title: &str) -> Result<&str, TicketError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(TicketError::Invalid);
    }
    Ok(clip(title, TITLE_LIMIT))
}

/// The longest prefix of `text` of at most `limit` bytes that ends on a character boundary.
fn clip(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let end = (0..=limit)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    &text[..end]
}

#[cfg(test)]
mod tests;
