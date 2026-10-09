//! Canonical WebSocket payloads for Paseo Agent runtime directory and metadata lifecycle.

use std::collections::BTreeMap;

use domain::workspace::protocol::workspace::ProjectPlacementPayload;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::agent_config::NullableSetting;

/// Paseo Agent lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentStatus {
    /// Provider construction has started.
    Initializing,
    /// No provider turn is active.
    Idle,
    /// A provider turn is active.
    Running,
    /// The latest provider operation failed.
    Error,
    /// Only a persisted snapshot remains.
    Closed,
}

/// Why a client should surface an Agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentAttentionReason {
    /// A turn finished.
    Finished,
    /// A turn failed.
    Error,
    /// A provider permission is pending.
    Permission,
}

/// Provider capability flags attached to one Agent snapshot.
///
/// Paseo deliberately permits provider-specific boolean keys in addition to its common flags.
pub(crate) type AgentCapabilityFlags = BTreeMap<String, bool>;

/// Provider resume handle exposed only when its provider is installed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentPersistenceHandle {
    /// Provider identifier.
    provider: String,
    /// Provider session identifier.
    session_id: String,
    /// Optional provider-native handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    native_handle: Option<Value>,
    /// Provider-owned metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<BTreeMap<String, Value>>,
}

/// Last runtime facts reported by a provider.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRuntimeInfo {
    /// Provider identifier.
    pub(crate) provider: String,
    /// Current provider session.
    pub(crate) session_id: Option<String>,
    /// Effective provider model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) model: Option<String>,
    /// Effective thinking option.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thinking_option_id: Option<String>,
    /// Effective provider mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mode_id: Option<String>,
    /// Provider-owned runtime values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extra: Option<BTreeMap<String, Value>>,
}

/// Paseo-compatible projection of a durable Agent snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentSnapshotPayload {
    /// Stable Agent identity.
    pub(crate) id: String,
    /// Provider identifier.
    pub(crate) provider: String,
    /// Session working directory.
    pub(crate) cwd: String,
    /// Owning workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workspace_id: Option<String>,
    /// Configured model.
    pub(crate) model: Option<String>,
    /// Provider feature descriptors.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) features: Vec<Value>,
    /// Configured thinking option.
    pub(crate) thinking_option_id: Option<String>,
    /// Effective thinking option.
    pub(crate) effective_thinking_option_id: Option<String>,
    /// Creation timestamp.
    pub(crate) created_at: String,
    /// Latest update timestamp.
    pub(crate) updated_at: String,
    /// Latest user-message timestamp.
    pub(crate) last_user_message_at: Option<String>,
    /// Durable lifecycle state.
    pub(crate) status: AgentStatus,
    /// Active turn; persisted records have no active turn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active_turn: Option<Value>,
    /// Provider capability projection.
    pub(crate) capabilities: AgentCapabilityFlags,
    /// Current provider mode.
    pub(crate) current_mode_id: Option<String>,
    /// Available provider modes.
    pub(crate) available_modes: Vec<Value>,
    /// Pending provider permissions.
    pub(crate) pending_permissions: Vec<Value>,
    /// Durable provider identity, if its provider is available.
    pub(crate) persistence: Option<AgentPersistenceHandle>,
    /// Last provider runtime facts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) runtime_info: Option<AgentRuntimeInfo>,
    /// Latest provider-reported usage, retained across disconnects and native resume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_usage: Option<crate::protocol::usage::AgentUsage>,
    /// Last provider error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) last_error: Option<String>,
    /// User-visible title.
    pub(crate) title: Option<String>,
    /// String labels.
    pub(crate) labels: BTreeMap<String, String>,
    /// Whether the Agent asks for attention.
    pub(crate) requires_attention: bool,
    /// Attention category.
    pub(crate) attention_reason: Option<AgentAttentionReason>,
    /// Attention timestamp.
    pub(crate) attention_timestamp: Option<String>,
    /// Soft-delete timestamp.
    pub(crate) archived_at: Option<String>,
    /// Whether the referenced provider is unavailable.
    pub(crate) provider_unavailable: bool,
}

/// Agent directory filters.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryFilter {
    /// Require exact key/value label matches.
    #[serde(default)]
    pub(crate) labels: Option<BTreeMap<String, String>>,
    /// Restrict placement to project keys.
    #[serde(default)]
    pub(crate) project_keys: Option<Vec<String>>,
    /// Restrict lifecycle states.
    #[serde(default)]
    pub(crate) statuses: Option<Vec<AgentStatus>>,
    /// Include soft-deleted Agents.
    #[serde(default)]
    pub(crate) include_archived: Option<bool>,
    /// Restrict attention state.
    #[serde(default)]
    pub(crate) requires_attention: Option<bool>,
    /// Restrict configured thinking option; explicit null means provider default.
    #[serde(default)]
    pub(crate) thinking_option_id: NullableSetting,
}

/// Sortable Agent directory fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentSortKey {
    /// Attention and lifecycle priority.
    StatusPriority,
    /// Creation timestamp.
    CreatedAt,
    /// Update timestamp.
    UpdatedAt,
    /// Case-insensitive title.
    Title,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// One Agent directory sort term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) struct AgentSort {
    /// Sort field.
    pub(crate) key: AgentSortKey,
    /// Sort direction.
    pub(crate) direction: SortDirection,
}

/// Cursor page request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct AgentPageRequest {
    /// Page size, from one through 200.
    pub(crate) limit: usize,
    /// Opaque continuation cursor.
    #[serde(default)]
    pub(crate) cursor: Option<String>,
}

/// Active directory read request.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentListRequest {
    /// Paseo accepts only the literal `active`.
    #[serde(default)]
    pub(crate) scope: Option<String>,
    /// Directory filters.
    #[serde(default)]
    pub(crate) filter: Option<AgentDirectoryFilter>,
    /// Ordered sort terms.
    #[serde(default)]
    pub(crate) sort: Option<Vec<AgentSort>>,
    /// Cursor page.
    #[serde(default)]
    pub(crate) page: Option<AgentPageRequest>,
    /// Connection-owned live directory subscription.
    #[serde(default)]
    pub(crate) subscribe: Option<domain::workspace::protocol::directory::SubscriptionRequest>,
    /// Latest-state synchronization checkpoint.
    #[serde(default)]
    pub(crate) sync: Option<domain::directory_sync::Cursor>,
}

/// Historical Agent directory request.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentHistoryRequest {
    /// Directory filters; archived records are included by default.
    #[serde(default)]
    pub(crate) filter: Option<AgentDirectoryFilter>,
    /// Case-insensitive query over title and placement names.
    #[serde(default)]
    pub(crate) search: Option<String>,
    /// Ordered sort terms.
    #[serde(default)]
    pub(crate) sort: Option<Vec<AgentSort>>,
    /// Cursor page.
    #[serde(default)]
    pub(crate) page: Option<AgentPageRequest>,
}

/// One directory row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryEntry {
    /// Agent runtime snapshot.
    pub(crate) agent: AgentSnapshotPayload,
    /// Project/workspace placement.
    pub(crate) project: ProjectPlacementPayload,
}

/// Directory page metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentPageInfo {
    /// Next page cursor.
    pub(crate) next_cursor: Option<String>,
    /// Cursor used to read this page.
    pub(crate) prev_cursor: Option<String>,
    /// Whether another page exists.
    pub(crate) has_more: bool,
}

/// Agent directory or history result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDirectoryResult {
    /// Matching rows.
    pub(crate) entries: Vec<AgentDirectoryEntry>,
    /// Page metadata.
    pub(crate) page_info: AgentPageInfo,
}

/// Resolve one Agent by full ID, unique prefix, or exact title.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentGetRequest {
    /// Agent identifier accepted by Paseo resolution rules.
    pub(crate) agent_id: String,
}

/// One Agent lookup result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct AgentGetResult {
    /// Agent snapshot, or null on lookup failure.
    pub(crate) agent: Option<AgentSnapshotPayload>,
    /// Placement, when the Agent belongs to a known workspace.
    pub(crate) project: Option<ProjectPlacementPayload>,
    /// Safe lookup error.
    pub(crate) error: Option<String>,
}

/// Update Agent metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentUpdateRequest {
    /// Full Agent identity.
    pub(crate) agent_id: String,
    /// Nonempty title after trimming.
    #[serde(default)]
    pub(crate) name: Option<String>,
    /// Replacement labels when nonempty.
    #[serde(default)]
    pub(crate) labels: Option<BTreeMap<String, String>>,
}

/// Common metadata action result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentActionResult {
    /// Full Agent identity.
    pub(crate) agent_id: String,
    /// Whether the operation was accepted.
    pub(crate) accepted: bool,
    /// Safe business error.
    pub(crate) error: Option<String>,
}

/// Request targeting one full Agent identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentIdRequest {
    /// Full Agent identity.
    pub(crate) agent_id: String,
}

/// Archive result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentArchiveResult {
    /// Full Agent identity.
    pub(crate) agent_id: String,
    /// Archive timestamp.
    pub(crate) archived_at: String,
}

/// Permanent deletion result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentDeleteResult {
    /// Full Agent identity.
    pub(crate) agent_id: String,
}

/// One or several Agent identities accepted by clear-attention.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum AgentIdSelection {
    /// One Agent identity.
    One(String),
    /// Several Agent identities.
    Many(Vec<String>),
}

/// Clear attention request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentAttentionClearRequest {
    /// One Agent identity or an array of identities.
    pub(crate) agent_id: AgentIdSelection,
}

/// Clear attention result.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentAttentionClearResult {
    /// Original selection.
    pub(crate) agent_id: AgentIdSelection,
    /// Updated snapshots.
    pub(crate) agents: Vec<AgentSnapshotPayload>,
}

/// Batch close request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentItemsCloseRequest {
    /// Agents to archive independently.
    #[serde(default)]
    pub(crate) agent_ids: Vec<String>,
    /// Terminal identities reserved for the terminal phase.
    #[serde(default)]
    pub(crate) terminal_ids: Vec<String>,
}

/// One successful Agent archive in a batch close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClosedAgentResult {
    /// Agent identity.
    pub(crate) agent_id: String,
    /// Archive timestamp.
    pub(crate) archived_at: String,
}

/// Batch close result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AgentItemsCloseResult {
    /// Successfully archived Agents; failures are omitted as in Paseo.
    pub(crate) agents: Vec<ClosedAgentResult>,
    /// Terminal results. This phase accepts only an empty terminal request.
    pub(crate) terminals: Vec<Value>,
}

#[cfg(test)]
mod tests;
