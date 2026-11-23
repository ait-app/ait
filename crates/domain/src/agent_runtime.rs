//! Persisted Paseo Agent runtime records.
//!
//! The shape follows `StoredAgentRecord` from the pinned Paseo server. It is separate from the
//! independent server's versioned Agent preset catalog in [`crate::agent`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Provider-owned execution observed through a registered root Agent.
pub mod observation;
/// Blocking storage contract for the persisted runtime record.
pub mod registry;

/// Lifecycle states persisted by Paseo for an Agent runtime instance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    /// Provider construction has started.
    Initializing,
    /// The Agent is ready and has no active turn.
    Idle,
    /// A provider turn is active.
    Running,
    /// The latest provider operation failed.
    Error,
    /// No live provider process owns the stored snapshot.
    #[default]
    Closed,
}

/// Reason an Agent asks a client for attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAttentionReason {
    /// A turn finished while the Agent was not focused.
    Finished,
    /// A turn failed.
    Error,
    /// Provider execution is waiting for permission.
    Permission,
}

/// Serializable subset of one provider session configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAgentConfig {
    /// Selected provider mode.
    #[serde(default)]
    pub mode_id: Option<String>,
    /// Selected model.
    #[serde(default)]
    pub model: Option<String>,
    /// Selected thinking option.
    #[serde(default)]
    pub thinking_option_id: Option<String>,
    /// Provider feature values.
    #[serde(default)]
    pub feature_values: Option<BTreeMap<String, Value>>,
    /// Provider-specific JSON options.
    #[serde(default)]
    pub provider_options: Option<BTreeMap<String, Value>>,
    /// Tool approval policy.
    #[serde(default)]
    pub tool_policy: Option<Value>,
    /// Optional provider system prompt.
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// MCP server definitions owned by the provider session.
    #[serde(default)]
    pub mcp_servers: Option<BTreeMap<String, Value>>,
}

/// Provider-owned durable session identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPersistenceHandle {
    /// Provider identifier.
    pub provider: String,
    /// Provider session identifier.
    pub session_id: String,
    /// Optional provider-native resume handle.
    #[serde(default)]
    pub native_handle: Option<Value>,
    /// Provider-owned resume metadata.
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, Value>>,
}

/// Last runtime facts reported by the provider adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAgentRuntimeInfo {
    /// Provider identifier.
    pub provider: String,
    /// Current provider session, when established.
    pub session_id: Option<String>,
    /// Effective model reported by the provider.
    #[serde(default)]
    pub model: Option<String>,
    /// Effective thinking option reported by the provider.
    #[serde(default)]
    pub thinking_option_id: Option<String>,
    /// Effective mode reported by the provider.
    #[serde(default)]
    pub mode_id: Option<String>,
    /// Additional provider-owned runtime facts.
    #[serde(default)]
    pub extra: Option<BTreeMap<String, Value>>,
}

/// Provenance of a system-supplied title, used to protect manual edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TitleOrigin {
    /// Provisional text may be replaced by a successful metadata generation.
    Prompt,
    /// A generated title must not be regenerated on every subsequent message.
    Generated,
}

/// Explicit identity of a preset in the provider's effective store for one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPresetReference {
    /// Name in the provider's preset store.
    pub name: String,
    /// Store layer, one of `ait`, `global`, or `project`.
    pub source: String,
    /// Canonical directory used to resolve the effective preset layer.
    pub cwd: String,
    /// Agent directory bound to this selection, distinct for newly created worktrees.
    pub agent_cwd: String,
}

/// Preset identity and the independent role selections it temporarily replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPresetState {
    /// Selected identity; absent after a manual role edit makes the selection custom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<ModelPresetReference>,
    /// Independent role and fallback overrides to restore on leaving the preset.
    #[serde(default)]
    pub baseline: BTreeMap<String, Value>,
    /// Independent thinking selection from before the first preset was selected.
    #[serde(default)]
    pub baseline_thinking: Option<String>,
    /// Fields still owned by the preset and eligible for baseline restoration.
    #[serde(default)]
    pub owned: std::collections::BTreeSet<String>,
}

/// Durable Agent runtime snapshot translated from Paseo `StoredAgentRecord`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedAgentRuntimeRecord {
    /// Stable Agent runtime identity.
    pub id: String,
    /// Provider identifier.
    pub provider: String,
    /// Session working directory.
    pub cwd: String,
    /// Owning workspace, when the Agent has placement.
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Creation timestamp string.
    pub created_at: String,
    /// Latest snapshot timestamp string.
    pub updated_at: String,
    /// Latest activity timestamp string.
    #[serde(default)]
    pub last_activity_at: Option<String>,
    /// Latest user-message timestamp string.
    #[serde(default)]
    pub last_user_message_at: Option<String>,
    /// User-visible title.
    #[serde(default)]
    pub title: Option<String>,
    /// Automatic title provenance; absent for explicit, native or legacy titles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_origin: Option<TitleOrigin>,
    /// Arbitrary string labels, including Paseo delegation metadata.
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    /// Last durable lifecycle state.
    #[serde(default)]
    pub last_status: AgentRuntimeStatus,
    /// Last provider mode.
    #[serde(default)]
    pub last_mode_id: Option<String>,
    /// Serializable provider configuration.
    #[serde(default)]
    pub config: Option<StoredAgentConfig>,
    /// Explicit preset identity and reversible role/fallback ownership, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_preset: Option<ModelPresetState>,
    /// Monotonic desired-config revision, used to reject stale preset reads.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub config_revision: u64,
    /// Last provider runtime facts.
    #[serde(default)]
    pub runtime_info: Option<StoredAgentRuntimeInfo>,
    /// Provider-defined feature descriptors.
    #[serde(default)]
    pub features: Vec<Value>,
    /// Durable provider resume identity.
    #[serde(default)]
    pub persistence: Option<AgentPersistenceHandle>,
    /// Last provider error safe for client display.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Whether a client should surface this Agent.
    #[serde(default)]
    pub requires_attention: bool,
    /// Attention category.
    #[serde(default)]
    pub attention_reason: Option<AgentAttentionReason>,
    /// Attention timestamp string.
    #[serde(default)]
    pub attention_timestamp: Option<String>,
    /// Internal runtime records never appear in the public directory.
    #[serde(default)]
    pub internal: bool,
    /// Soft-delete timestamp string.
    #[serde(default)]
    pub archived_at: Option<String>,
    /// Optional daemon execution ownership metadata.
    #[serde(default)]
    pub owner: Option<Value>,
    /// Read-only native child binding; absent for sessions managed by Ait.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<observation::ObservedAgent>,
}

// Serde supplies a reference to fields in skip_serializing_if predicates.
#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[cfg(test)]
mod tests;
