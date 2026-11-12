//! Read-only native session facts; the provider remains the owner of its history.

use domain::agent_runtime::StoredAgentConfig;
use serde::Serialize;

use crate::protocol::timeline::NativeItem;

/// Payload-free context for a failed native history page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error(
    "History reload failed: {method}, page {page}: {failure}; accumulated display bytes={bytes}. Native history is unchanged. Retry after resolving the reported limit or provider error, or continue in the native client."
)]
pub struct HistoryError {
    /// Native read method, supplied by the adapter rather than the response.
    pub method: &'static str,
    /// One-based page number.
    pub page: usize,
    /// Serialized display bytes retained before this failure.
    pub bytes: usize,
    /// Limit, protocol or native request failure without conversation contents.
    pub failure: HistoryFailure,
}

/// Distinguish display budgets from malformed history and failed RPCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HistoryFailure {
    /// A named bounded resource exceeded its allowance.
    #[error("{resource} limit exceeded (limit={limit}, actual={actual})")]
    Limit {
        /// Adapter-owned resource name and unit.
        resource: &'static str,
        /// Maximum accepted value.
        limit: usize,
        /// Observed value including the rejected entry.
        actual: usize,
    },
    /// Adapter-owned explanation of invalid protocol data.
    #[error("invalid history protocol: {0}")]
    Protocol(&'static str),
    /// A native request failed; this is never an unsupported-method fallback.
    #[error("native history RPC failed: {0}")]
    Rpc(&'static str),
}

/// Paseo-compatible discovery descriptor, without local native storage paths or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDescriptor {
    /// Registered adapter identity.
    pub provider_id: String,
    /// User-facing adapter label.
    pub provider_label: String,
    /// Native session identity.
    pub provider_handle_id: String,
    /// Native working directory.
    pub cwd: String,
    /// Native title, when present.
    pub title: Option<String>,
    /// First user text, when supplied by native discovery.
    pub first_prompt_preview: Option<String>,
    /// Last user text, when supplied by native discovery.
    pub last_prompt_preview: Option<String>,
    /// Native update timestamp in RFC3339.
    pub last_activity_at: String,
}

/// Bounded native discovery options. Final import and query filtering belongs to the service.
#[derive(Debug, Clone)]
pub struct ListOptions {
    /// Canonical exact cwd filter; none discovers across directories.
    pub cwd: Option<String>,
    /// Maximum distinct sessions inspected, in the range 1..=4096.
    pub scan_limit: usize,
}

/// Working-directory spelling a provider's native session lookup expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CwdLookup {
    /// The canonical directory Ait stores for the Agent.
    #[default]
    Canonical,
    /// The spelling the client sent, before canonicalization.
    Requested,
}

/// Complete native facts used to validate an import or refresh before publishing host state.
#[derive(Debug, Clone)]
pub struct SessionHistory {
    /// Provider-owned resume facts for this identity, including an intentional fresh branch.
    pub resume_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// Immediate native parent for a provider-owned child, when present.
    pub parent_id: Option<String>,
    /// Native identity and display metadata.
    pub descriptor: SessionDescriptor,
    /// Native creation timestamp in RFC3339, or observation time when the protocol omits it.
    pub created_at: String,
    /// Native model and effort, constrained by this adapter's execution policy.
    pub config: StoredAgentConfig,
    /// True when the provider reports an active or incomplete turn.
    pub active: bool,
    /// Ordered completed display items.
    pub entries: Vec<NativeItem>,
}
