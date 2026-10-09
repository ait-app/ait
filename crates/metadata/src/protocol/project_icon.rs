//! Project icon WebSocket payloads translated from Paseo.

use serde::{Deserialize, Serialize};

/// Client-owned icon source. URL fetching is deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ProjectIconSource {
    /// Remove custom bytes and resume automatic discovery.
    Automatic,
    /// Validate and store client-provided base64 image bytes.
    Upload {
        /// Base64-encoded image bytes.
        data: String,
    },
}

/// Set or clear a custom icon for a registered project.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectIconSetRequest {
    /// Project identity.
    pub(crate) project_id: String,
    /// Automatic mode or uploaded bytes.
    pub(crate) source: ProjectIconSource,
}

/// Read the effective custom or automatically discovered icon.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectIconGetRequest {
    /// Project identity.
    pub(crate) project_id: String,
}

/// Base64 project icon returned to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectIconPayload {
    /// Base64-encoded image bytes.
    pub(crate) data: String,
    /// MIME type detected by the server.
    pub(crate) mime_type: String,
}

/// Project icon mutation outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectIconSetResult {
    /// Project identity.
    pub(crate) project_id: String,
    /// Whether the custom/automatic selection was persisted.
    pub(crate) accepted: bool,
    /// Safe business error.
    pub(crate) error: Option<String>,
}

/// Effective project icon read outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectIconGetResult {
    /// Project identity.
    pub(crate) project_id: String,
    /// Effective icon, or null when automatic discovery found none.
    pub(crate) icon: Option<ProjectIconPayload>,
    /// Safe business error.
    pub(crate) error: Option<String>,
}

#[cfg(test)]
mod tests;
