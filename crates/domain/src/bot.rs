//! Workspace-owned Bots, exclusive channels and durable conversation receipts.

use serde::{Deserialize, Serialize};

use crate::{BotId, BotTurnId, ChannelId, ConversationId};

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

/// Bot configuration, independent of any transport or native session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bot {
    /// Stable workspace tab identity.
    pub id: BotId,
    /// Owning workspace, which must be active for execution.
    pub workspace_id: String,
    /// Display name.
    pub name: String,
    /// Persona and behavior instructions.
    pub instructions: String,
    /// Provider selection for new conversations.
    pub execution: ExecutionConfig,
    /// Built-in Ait channel allocated atomically with the Bot.
    pub builtin_channel_id: ChannelId,
    /// Whether this Bot admits incoming messages.
    pub enabled: bool,
    /// Archive timestamp, retaining history and memory.
    pub archived_at: Option<String>,
    /// UTC creation timestamp.
    pub created_at: String,
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
    /// Hidden provider runtime identity, rotated when resetting the conversation.
    pub session_id: String,
    /// Conversation generation, incremented by an explicit reset.
    pub generation: u64,
    /// Native configuration fixed for this generation.
    pub execution: ExecutionConfig,
    /// Persona fixed for this generation.
    pub instructions: String,
    /// Next ordered input sequence; gaps after failed persistence are permitted.
    pub next_sequence: u64,
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
