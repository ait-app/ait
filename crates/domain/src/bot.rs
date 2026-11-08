//! Workspace-owned Bots, exclusive channels and durable conversation receipts.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use self::permissions::BotPermissions;

use crate::{BotId, BotTurnId, ChannelId, ConversationId};

pub mod permissions;

/// Reserved label carrying the Bot scope of an Agent or schedule.
pub const BOT_SCOPE_LABEL: &str = "ait.bot.scope";

/// Label namespace no MCP caller or guest may write; `BOT_SCOPE_LABEL` lives in it.
pub const BOT_LABEL_PREFIX: &str = "ait.bot.";

/// Native execution selection; credentials remain provider-owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionConfig {
    /// Registered provider identity.
    pub provider: String,
    /// Optional native model selection.
    pub model: Option<String>,
    /// Optional native permission mode.
    pub mode_id: Option<String>,
    /// Optional reasoning selection.
    pub thinking: Option<String>,
}

/// Why a Workspace Bot stopped admitting work while keeping its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// The user paused the Bot explicitly.
    User,
    /// The Workspace directory disappeared; the user must resume explicitly.
    DirectoryMissing,
}

/// Lifecycle of a Workspace's single Bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BotState {
    /// Admits input and runs turns.
    Active,
    /// Keeps configuration and history but admits no input.
    Paused {
        /// Cause of the pause.
        reason: PauseReason,
    },
    /// Permanently replaced or removed; history is kept for audit only.
    Retired {
        /// UTC retirement timestamp.
        at: String,
    },
}

/// A Workspace's Bot configuration, independent of any transport or native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bot {
    /// Internal storage and generation key; clients address the Bot by Workspace.
    pub id: BotId,
    /// Owning workspace, which must be active for execution.
    pub workspace_id: String,
    /// Provider selection for new conversations.
    pub execution: ExecutionConfig,
    /// Built-in Ait channel allocated atomically with the Bot.
    pub builtin_channel_id: ChannelId,
    /// Lifecycle state; at most one non-retired Bot exists per Workspace.
    pub state: BotState,
    /// UTC creation timestamp.
    pub created_at: String,
    /// User-configured MCP tool groups and per-provider mode cap.
    #[serde(default)]
    pub permissions: BotPermissions,
    /// Unarchived worktree Workspaces this Bot created before Bots lost `create_workspace`; they
    /// stay in scope so the Agents already working there keep their access.
    #[serde(default)]
    pub workspaces: BTreeSet<String>,
}

/// Transport-specific configuration; secret values are never part of this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChannelConfig {
    /// Ait's authenticated Bot tab.
    Builtin,
    /// Telegram Bot API long polling.
    Telegram {
        /// Runtime environment reference, for example `env:AIT_SERVER_CREDENTIAL_TELEGRAM`.
        token_ref: String,
        /// Explicit Telegram user IDs allowed to invoke this Bot.
        allowed_senders: Vec<String>,
        /// Whether mentioned group messages are accepted.
        #[serde(default)]
        allow_groups: bool,
    },
    /// Feishu self-built application's persistent WebSocket connection.
    Feishu {
        /// Public application identity.
        app_id: String,
        /// Runtime environment reference to the application secret.
        secret_ref: String,
        /// Explicit tenant-scoped sender open IDs.
        allowed_senders: Vec<String>,
        /// Whether mentioned group messages are accepted.
        #[serde(default)]
        allow_groups: bool,
    },
}

/// An account instance owned exclusively by one Bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    /// Stable channel identity.
    pub id: ChannelId,
    /// Exclusive owner.
    pub bot_id: BotId,
    /// Transport configuration.
    pub config: ChannelConfig,
    /// Whether receiving is enabled.
    pub enabled: bool,
    /// Telegram's next durable update offset.
    pub cursor: Option<i64>,
    /// Deleted channels retain their routing identity for historical receipts.
    #[serde(default)]
    pub deleted: bool,
}

/// Scope verified by Provider before every private execution operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionOwner {
    /// Owning Bot.
    pub bot_id: BotId,
    /// Conversation isolated from other peers and channels.
    pub conversation_id: ConversationId,
    /// Workspace placement.
    pub workspace_id: String,
}

/// Durable routing and private native-session identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Conversation {
    /// Public Bot conversation identity.
    pub id: ConversationId,
    /// Owner.
    pub bot_id: BotId,
    /// Receiving and replying channel.
    pub channel_id: ChannelId,
    /// External chat or built-in owner route.
    pub peer: String,
    /// Sender scope; group participants do not share native history by default.
    pub sender: String,
    /// Optional platform thread identity.
    pub thread: Option<String>,
    /// Hidden Agent identity, published to clients for read-only inspection and rotated when
    /// resetting the conversation.
    pub session_id: String,
    /// Conversation generation, incremented by an explicit reset.
    pub generation: u64,
    /// Native configuration fixed for this generation.
    pub execution: ExecutionConfig,
    /// Next ordered input sequence; gaps after failed persistence are permitted.
    pub next_sequence: u64,
    /// Unused: earlier builds recorded the digest of the instructions last prefixed to an input.
    /// Instructions delivered as a turn prefix now ride on every input, so nothing reads or
    /// writes this; it remains so conversations stored by those builds still load.
    #[serde(default)]
    pub instructions_digest: Option<String>,
}

/// Durable execution state; uncertain work is never replayed automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    /// Input committed and waiting for its conversation's previous turn.
    Queued,
    /// Native admission may have started.
    Sending,
    /// Native turn acknowledged and executing.
    Running,
    /// Waiting for an explicit local permission decision.
    NeedsApproval,
    /// Final answer committed.
    Completed,
    /// Execution failed definitively.
    Failed,
    /// Execution or delivery outcome needs explicit reconciliation.
    Uncertain,
    /// Cancellation acknowledged.
    Cancelled,
}

/// External reply receipt, independent of execution completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    /// Reply remains available for transmission.
    Pending,
    /// Sending began; restart must not blindly repeat it.
    Sending,
    /// Platform acknowledged the reply, or the built-in client can read it.
    Delivered,
    /// Platform acknowledgement was lost or ambiguous.
    Uncertain,
}

/// Durable, idempotent user input and its final reply.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Turn {
    /// Receipt identity returned before native execution.
    pub id: BotTurnId,
    /// Isolated conversation.
    pub conversation_id: ConversationId,
    /// Stable input key, scoped to this channel.
    pub message_id: String,
    /// Monotonic order within the conversation.
    pub sequence: u64,
    /// Conversation generation owning this input.
    pub generation: u64,
    /// Original input text.
    pub text: String,
    /// Original platform message identity used as the reply target.
    pub reply_to: Option<String>,
    /// Committed final assistant answer.
    pub reply: Option<String>,
    /// Pending native permission requests, visible only through the owning Bot interface.
    #[serde(default)]
    pub pending_permissions: Vec<serde_json::Value>,
    /// Execution outcome.
    pub status: TurnStatus,
    /// Transport delivery outcome.
    pub delivery: DeliveryStatus,
    /// Safe diagnostic, without raw transport/provider errors.
    pub error: Option<String>,
    /// UTC admission timestamp.
    pub created_at: String,
}

/// Atomically persisted configuration catalog; histories live in separate files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    /// Workspace-owned Bots.
    pub bots: Vec<Bot>,
    /// Exclusive channel instances.
    pub channels: Vec<Channel>,
}

#[cfg(test)]
mod tests;
