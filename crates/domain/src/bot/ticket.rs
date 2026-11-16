//! Units of work a Bot delegates to Agents: tickets, the Agents assigned to them and their
//! reports, with the ticket status machine (ADR-274).
//!
//! These records are new, so they accept unknown fields: a later daemon can extend them without
//! making an earlier one refuse the Bot store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::bot::notification::{Notification, NotificationKind};
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
            | NotificationKind::TicketFailed => return,
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
    /// hands the wait to the next blocked worker, if any. Returns the report's sequence.
    ///
    /// # Errors
    ///
    /// Returns [`TicketError::Closed`] for a closed ticket.
    pub fn report(
        &mut self,
        agent_id: &str,
        kind: WorkerReportKind,
        text: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, TicketError> {
        if self.status.is_closed() {
            return Err(TicketError::Closed);
        }
        let status = match kind {
            WorkerReportKind::Blocked => self.block(agent_id, at),
            WorkerReportKind::Progress | WorkerReportKind::Done | WorkerReportKind::Failed => {
                self.unblock(agent_id)
            }
        };
        Ok(self.record(Report {
            sequence: 0,
            at,
            source: ReportSource::Agent,
            kind: kind.report_kind(),
            agent_id: Some(agent_id.to_owned()),
            text: clip(text, TEXT_LIMIT).to_owned(),
            status,
        }))
    }

    /// Whether the end of an Agent's turn, `notification`, would only repeat a wake-up the Bot
    /// already had for the same event: that Agent's latest worker report recorded since its
    /// previous turn end on this ticket was made in the turn's window and is an outcome
    /// (`blocked`, `done` or `failed`; only `failed` for a failed turn). The window runs from the
    /// later of the turn's start, as the daemon dated it, and that previous turn end, to the
    /// turn's end. Permission requests and turns whose start is unknown are never spared.
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
            | NotificationKind::TicketFailed => return false,
        };
        let (Some(agent), Some(started)) = (
            notification.agent_id.as_deref(),
            notification.turn_started_at,
        ) else {
            return false;
        };
        // Reports are appended in admission order, so those after the Agent's latest recorded
        // turn end belong to a later turn.
        let latest = self
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
            .find(|report| report.source == ReportSource::Agent);
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
                    | ReportKind::Status => false,
                }
        })
    }

    /// Close this ticket on Ait's behalf at `at` because every Agent working on it is gone, with
    /// `reason` as the outcome and the text of a system report. An open ticket closes as `done`
    /// only when every assigned Agent, and every other worker that reported, last reported
    /// `done` since the ticket was last reopened; otherwise, and always for a waiting ticket,
    /// which nobody can answer any more, as `cancelled`. Closed tickets and tickets without
    /// assignments are left alone (`None`); otherwise returns the new status.
    pub fn close_finished(&mut self, reason: &str, at: DateTime<Utc>) -> Option<TicketStatus> {
        if self.status.is_closed() || self.assignments.is_empty() {
            return None;
        }
        let target = if self.status == TicketStatus::Open && self.finished_since_reopened() {
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
    /// closed ticket with another status. A refused update changes nothing.
    pub fn update(
        &mut self,
        source: ReportSource,
        update: &TicketUpdate,
        at: DateTime<Utc>,
    ) -> Result<bool, TicketError> {
        let empty = update.status.is_none() && update.title.is_none() && update.note.is_none();
        let closing = update.status.is_some_and(TicketStatus::is_closed);
        if empty
            || update.status == Some(TicketStatus::Waiting)
            || (update.outcome.is_some() && !closing)
        {
            return Err(TicketError::Invalid);
        }
        let title = update.title.as_deref().map(valid_title).transpose()?;
        if let Some(target) = update.status
            && target.is_closed()
            && self.status.is_closed()
            && self.status != target
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
            if moved {
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
        Ok(changed)
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

    /// Remember that `agent_id` is blocked since `at` and make the ticket wait on it unless
    /// another worker's block already does; returns the status when the wait changed.
    fn block(&mut self, agent_id: &str, at: DateTime<Utc>) -> Option<TicketStatus> {
        if self.blocked.len() < ASSIGNMENT_LIMIT
            && !self.blocked.iter().any(|block| block.agent_id == agent_id)
        {
            self.blocked.push(Waiting {
                reason: WaitReason::Blocked,
                agent_id: agent_id.to_owned(),
                since: at,
            });
        }
        let held = self
            .waiting
            .as_ref()
            .is_some_and(|waiting| waiting.reason == WaitReason::Blocked);
        (!held).then(|| self.wait(WaitReason::Blocked, agent_id, at))
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
