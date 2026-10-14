//! Read-only native session facts; the provider remains the owner of its history.

use domain::agent_runtime::StoredAgentConfig;
use serde::Serialize;

use crate::protocol::timeline::NativeItem;

/// Paseo-compatible discovery descriptor, without local native storage paths or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionDescriptor {
    /// Registered adapter identity.
    pub(crate) provider_id: String,
    /// User-facing adapter label.
    pub(crate) provider_label: String,
    /// Native session identity.
    pub(crate) provider_handle_id: String,
    /// Native working directory.
    pub(crate) cwd: String,
    /// Native title, when present.
    pub(crate) title: Option<String>,
    /// First user text, when supplied by native discovery.
    pub(crate) first_prompt_preview: Option<String>,
    /// Last user text, when supplied by native discovery.
    pub(crate) last_prompt_preview: Option<String>,
    /// Native update timestamp in RFC3339.
    pub(crate) last_activity_at: String,
}

/// Bounded native discovery options. Final import and query filtering belongs to the service.
#[derive(Debug, Clone)]
pub(crate) struct ListOptions {
    /// Canonical exact cwd filter; none discovers across directories.
    pub(crate) cwd: Option<String>,
    /// Maximum distinct sessions inspected, in the range 1..=4096.
    pub(crate) scan_limit: usize,
}

/// Complete native facts used to validate an import or refresh before publishing host state.
#[derive(Debug, Clone)]
pub(crate) struct SessionHistory {
    /// Provider-owned resume facts for this identity, including an intentional fresh branch.
    pub(crate) resume_metadata: std::collections::BTreeMap<String, serde_json::Value>,
    /// Immediate native parent for a provider-owned child, when present.
    pub(crate) parent_id: Option<String>,
    /// Native identity and display metadata.
    pub(crate) descriptor: SessionDescriptor,
    /// Native creation timestamp in RFC3339, or observation time when the protocol omits it.
    pub(crate) created_at: String,
    /// Native model and effort, constrained by this adapter's execution policy.
    pub(crate) config: StoredAgentConfig,
    /// True when the provider reports an active or incomplete turn.
    pub(crate) active: bool,
    /// Ordered completed display items.
    pub(crate) entries: Vec<NativeItem>,
}
