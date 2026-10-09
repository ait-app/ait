//! Existing native session discovery, import, refresh and context export.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::timeline::Cursor;

/// Filters for recent sessions which have not been actively imported.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecentRequest {
    /// Optional exact working directory.
    pub(crate) cwd: Option<String>,
    /// Registered providers, or all when omitted.
    pub(crate) providers: Option<Vec<String>>,
    /// Inclusive RFC3339 activity boundary.
    pub(crate) since: Option<String>,
    /// Positive result limit, at most 200; defaults to 20.
    pub(crate) limit: Option<usize>,
    /// Case-insensitive substring of title, prompt preview, handle or directory.
    pub(crate) query: Option<String>,
}

/// Import an existing provider handle. Legacy field aliases must agree when both are supplied.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImportRequest {
    /// Provider identity.
    pub(crate) provider_id: Option<String>,
    /// Legacy provider identity.
    pub(crate) provider: Option<String>,
    /// Native session identity.
    pub(crate) provider_handle_id: Option<String>,
    /// Legacy native identity.
    pub(crate) session_id: Option<String>,
    /// Existing absolute directory, checked against the native session.
    pub(crate) cwd: String,
    /// Explicit matching Workspace; omitted uses metadata's directory opening service.
    pub(crate) workspace_id: Option<String>,
    /// User-visible Agent labels.
    #[serde(default)]
    pub(crate) labels: BTreeMap<String, String>,
}

/// Select all context, or an inclusive timeline boundary.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ForkRequest {
    /// Full Agent ID, unique prefix or title.
    pub(crate) agent_id: String,
    /// Preferred inclusive sequence boundary.
    pub(crate) boundary_cursor: Option<Cursor>,
    /// Alternative inclusive assistant-message boundary.
    pub(crate) boundary_message_id: Option<String>,
}
