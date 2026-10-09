//! Workspace Agent attention payloads.

use serde::{Deserialize, Serialize};

/// One or several Workspace identities accepted by clear-attention.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum WorkspaceIdSelection {
    /// One Workspace identity.
    One(String),
    /// Several independently processed Workspace identities.
    Many(Vec<String>),
}

/// Clear non-permission Agent attention for one or several Workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceClearAttentionRequest {
    /// Workspace identity or batch.
    pub(crate) workspace_id: WorkspaceIdSelection,
}

/// Per-Workspace clear-attention result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceClearAttentionItem {
    /// Requested Workspace identity.
    pub(crate) workspace_id: String,
    /// Agents whose attention was cleared.
    pub(crate) cleared_agent_ids: Vec<String>,
    /// Whether this Workspace completed without an error.
    pub(crate) success: bool,
    /// Inline error text.
    pub(crate) error: Option<String>,
}

/// Aggregate clear-attention response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceClearAttentionResult {
    /// Original singular or batch selection.
    pub(crate) workspace_id: WorkspaceIdSelection,
    /// Flattened cleared Agent identities.
    pub(crate) cleared_agent_ids: Vec<String>,
    /// One result per requested Workspace.
    pub(crate) results: Vec<WorkspaceClearAttentionItem>,
    /// True only when every Workspace succeeded.
    pub(crate) success: bool,
    /// Aggregate inline error text.
    pub(crate) error: Option<String>,
}

/// Mark the newest finished root Agent in a Workspace as unread.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMarkUnreadRequest {
    /// Active Workspace identity.
    pub(crate) workspace_id: String,
}

/// Mark-unread response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceMarkUnreadResult {
    /// Requested Workspace identity.
    pub(crate) workspace_id: String,
    /// Agent marked unread, or null on rejection.
    pub(crate) marked_agent_id: Option<String>,
    /// Whether the mutation completed.
    pub(crate) success: bool,
    /// Inline error text.
    pub(crate) error: Option<String>,
}

#[cfg(test)]
mod tests;
