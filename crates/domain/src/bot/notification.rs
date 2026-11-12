//! Workspace notifications delivered to a Bot, its durable log and inbox, and wake triggers.
//!
//! These records are new, so they accept unknown fields: a later daemon can extend them without
//! making an earlier one refuse the whole Bot store.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{BotId, BotTurnId, ConversationId};

/// What happened to the notification's subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// An Agent finished its turn.
    AgentFinished,
    /// An Agent's turn failed.
    AgentFailed,
    /// An Agent is waiting for a permission decision in Ait.
    AgentPermission,
    /// A schedule run succeeded.
    ScheduleSucceeded,
    /// A schedule run failed.
    ScheduleFailed,
    /// A terminal asked for attention.
    TerminalAttention,
}

/// One Workspace event offered to the Bots that may care about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Stable, deterministic identity used for deduplication, for example
    /// `agent:<agentId>:<attentionTimestamp>` or `schedule:<scheduleId>:<runId>`.
    pub id: String,
    /// Workspace where the event happened.
    pub workspace_id: String,
    /// Root Workspace of the Bot whose scope started the subject, if any.
    #[serde(default)]
    pub scope: Option<String>,
    /// Kind of event.
    pub kind: NotificationKind,
    /// Subject Agent.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Subject schedule.
    #[serde(default)]
    pub schedule_id: Option<String>,
    /// Subject schedule run.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Subject terminal.
    #[serde(default)]
    pub terminal_id: Option<String>,
    /// Short human title.
    pub title: String,
    /// Optional human summary, such as the Agent's last message.
    #[serde(default)]
    pub summary: Option<String>,
    /// When the event happened.
    pub at: DateTime<Utc>,
    /// Bot conversation that started the subject, if known.
    #[serde(default)]
    pub conversation_id: Option<ConversationId>,
}

/// Why an in-scope notification did not wake the Bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The target conversation used up its wake budget.
    OverBudget,
    /// The inbox was full when the notification arrived.
    InboxFull,
    /// The Bot was paused while the notification waited.
    Paused,
    /// A turn of the target conversation already read the Agent's state after this event.
    Observed,
    /// The target conversation cannot run until the user resets it.
    Blocked,
}

/// Whether and how a logged notification woke the Bot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum WakeStatus {
    /// Outside the Bot's wake scope; it only feeds the overview.
    Logged,
    /// Waiting in the inbox for a wake turn.
    Pending,
    /// Folded into a wake turn.
    Woke {
        /// Conversation that ran the wake turn.
        conversation_id: ConversationId,
        /// The wake turn.
        turn_id: BotTurnId,
    },
    /// In scope but kept in the log only.
    Skipped {
        /// Cause.
        reason: SkipReason,
    },
}

/// One admitted notification in a Bot's log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedNotification {
    /// Position in the Bot's log, increasing from 1.
    pub sequence: u64,
    /// When the Bot admitted it; coalescing windows start here.
    pub admitted_at: DateTime<Utc>,
    /// The notification.
    pub notification: Notification,
    /// Inbox state.
    pub wake: WakeStatus,
}

/// A Bot's durable notification state: the bounded recent log, whose pending entries form the
/// inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationLog {
    /// Owning Bot.
    pub bot_id: BotId,
    /// Sequence the next admitted notification receives.
    pub next_sequence: u64,
    /// Entries in sequence order.
    pub entries: Vec<LoggedNotification>,
}

impl NotificationLog {
    /// An empty log for `bot_id`.
    #[must_use]
    pub fn new(bot_id: BotId) -> Self {
        Self {
            bot_id,
            next_sequence: 1,
            entries: Vec::new(),
        }
    }

    /// Sequence of the newest admitted notification, or 0 while none was admitted.
    #[must_use]
    pub fn head(&self) -> u64 {
        self.next_sequence.saturating_sub(1)
    }
}

/// The notifications a wake turn was started for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wake {
    /// Folded notifications in log order.
    pub notifications: Vec<Notification>,
}

#[cfg(test)]
mod tests;
