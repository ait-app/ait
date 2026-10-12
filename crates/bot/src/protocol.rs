use domain::bot::{ChannelConfig, ExecutionConfig};
use domain::{BotId, BotTurnId, ChannelId, ConversationId};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Empty {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Create {
    pub(crate) bot_id: BotId,
    pub(crate) workspace_id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) instructions: String,
    pub(crate) execution: ExecutionConfig,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BotRequest {
    pub(crate) bot_id: BotId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct List {
    pub(crate) workspace_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Update {
    pub(crate) bot_id: BotId,
    pub(crate) name: Option<String>,
    pub(crate) instructions: Option<String>,
    pub(crate) execution: Option<ExecutionConfig>,
    pub(crate) enabled: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelCreate {
    pub(crate) bot_id: BotId,
    pub(crate) channel_id: ChannelId,
    pub(crate) config: ChannelConfig,
    #[serde(default)]
    pub(crate) enabled: bool,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelRequest {
    pub(crate) bot_id: BotId,
    pub(crate) channel_id: ChannelId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelUpdate {
    pub(crate) bot_id: BotId,
    pub(crate) channel_id: ChannelId,
    pub(crate) enabled: bool,
    pub(crate) config: Option<ChannelConfig>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConversationRequest {
    pub(crate) bot_id: BotId,
    pub(crate) conversation_id: ConversationId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Send {
    pub(crate) bot_id: BotId,
    pub(crate) conversation_id: ConversationId,
    pub(crate) message_id: String,
    pub(crate) text: String,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct History {
    pub(crate) bot_id: BotId,
    pub(crate) conversation_id: ConversationId,
    #[serde(default)]
    pub(crate) after_sequence: u64,
    pub(crate) limit: Option<usize>,
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TurnRequest {
    pub(crate) bot_id: BotId,
    pub(crate) turn_id: BotTurnId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DecisionRequest {
    pub(crate) bot_id: BotId,
    pub(crate) turn_id: BotTurnId,
    pub(crate) permission_id: String,
    pub(crate) response: Value,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryUpdate {
    pub(crate) bot_id: BotId,
    pub(crate) text: String,
    pub(crate) expected: String,
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, model::ErrorCode> {
    serde_json::from_value(value).map_err(|_| model::ErrorCode::InvalidMessage)
}
