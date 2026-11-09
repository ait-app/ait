use domain::bot::permissions::BotPermissions;
use domain::bot::{ChannelConfig, ExecutionConfig};
use domain::{BotTurnId, ChannelId, ConversationId};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Empty {}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Setup {
    pub(crate) workspace_id: String,
    pub(crate) execution: ExecutionConfig,
    #[serde(default)]
    pub(crate) clear_memory: bool,
    #[serde(default)]
    pub(crate) permissions: Option<BotPermissions>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceRequest {
    pub(crate) workspace_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Update {
    pub(crate) workspace_id: String,
    pub(crate) execution: ExecutionConfig,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PermissionsUpdate {
    pub(crate) workspace_id: String,
    pub(crate) permissions: BotPermissions,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelCreate {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) config: ChannelConfig,
    #[serde(default)]
    pub(crate) enabled: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelRequest {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChannelUpdate {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) enabled: bool,
    pub(crate) config: Option<ChannelConfig>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScanStart {
    pub(crate) workspace_id: String,
    #[serde(default)]
    pub(crate) allow_groups: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScanRequest {
    pub(crate) workspace_id: String,
    pub(crate) scan_id: Uuid,
}
/// Manually entered Feishu credentials; deliberately not `Debug` because it holds a secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FeishuConnect {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) app_id: String,
    pub(crate) app_secret: String,
    pub(crate) allowed_senders: Vec<String>,
    #[serde(default)]
    pub(crate) allow_groups: bool,
}
/// Manually entered QQ credentials; deliberately not `Debug` because it holds a secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QqConnect {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) app_id: String,
    pub(crate) app_secret: String,
    #[serde(default)]
    pub(crate) allow_groups: bool,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QqPairRequest {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
}
/// A pasted bot token; deliberately not `Debug` because it holds a secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TelegramPairStart {
    pub(crate) workspace_id: String,
    pub(crate) token: String,
    #[serde(default)]
    pub(crate) allow_groups: bool,
}
/// Pasted Photon credentials; deliberately not `Debug` because it holds a secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImessagePairStart {
    pub(crate) workspace_id: String,
    pub(crate) project_id: String,
    pub(crate) api_key: String,
    pub(crate) handle: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PairRequest {
    pub(crate) workspace_id: String,
    pub(crate) pairing_id: Uuid,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConversationRequest {
    pub(crate) workspace_id: String,
    pub(crate) conversation_id: ConversationId,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Send {
    pub(crate) workspace_id: String,
    pub(crate) conversation_id: ConversationId,
    pub(crate) message_id: String,
    pub(crate) text: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct History {
    pub(crate) workspace_id: String,
    pub(crate) conversation_id: ConversationId,
    #[serde(default)]
    pub(crate) after_sequence: u64,
    pub(crate) limit: Option<usize>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TurnRequest {
    pub(crate) workspace_id: String,
    pub(crate) turn_id: BotTurnId,
}
/// Input the Ait center posts into a Hub channel; `thread` is the center's conversation.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HubSend {
    pub(crate) workspace_id: String,
    pub(crate) channel_id: ChannelId,
    pub(crate) peer: String,
    pub(crate) sender: String,
    pub(crate) thread: String,
    pub(crate) message_id: String,
    pub(crate) text: String,
}
/// What the center reports after posting a Hub turn's reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HubDelivery {
    Delivered,
    Uncertain,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HubAck {
    pub(crate) workspace_id: String,
    pub(crate) turn_id: BotTurnId,
    pub(crate) status: HubDelivery,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MemoryUpdate {
    pub(crate) workspace_id: String,
    pub(crate) text: String,
    pub(crate) expected: String,
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, model::ErrorCode> {
    serde_json::from_value(value).map_err(|_| model::ErrorCode::InvalidMessage)
}
