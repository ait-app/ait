//! Provider discovery and snapshot requests.

use serde::Deserialize;
use serde_json::Value;

/// Provider-owned model and mode definitions, with no credentials or private diagnostics.
#[derive(Debug, Clone, Default)]
pub(crate) struct Details {
    /// Paseo model definitions fetched from the native provider.
    pub(crate) models: Vec<Value>,
    /// Modes actually supported by this server adapter.
    pub(crate) modes: Vec<Value>,
    /// Runtime toggles and selects supported by the adapter.
    pub(crate) features: Vec<Value>,
}

/// Select one installed Provider in an optional working directory.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListRequest {
    /// Provider identity.
    pub(crate) provider: String,
    /// Optional directory scope. Missing, null and blank select the global catalog.
    pub(crate) cwd: Option<String>,
}

/// Preview features for the current draft without creating or changing an Agent.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FeaturesRequest {
    /// Proposed provider, directory, model, mode and feature values.
    pub(crate) draft_config: super::agent_execution::SessionConfig,
}

/// Fetch cached discovery facts, optionally reusing an unchanged snapshot.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SnapshotRequest {
    /// Scope used by native provider configuration lookup.
    pub(crate) cwd: Option<String>,
    /// Previously observed content hash.
    pub(crate) if_none_match: Option<String>,
}

/// Refresh all installed Providers, or a selected subset, for one directory.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefreshRequest {
    /// Scope used by native provider configuration lookup.
    pub(crate) cwd: Option<String>,
    /// Omitted means every registered adapter.
    pub(crate) providers: Option<Vec<String>>,
}
