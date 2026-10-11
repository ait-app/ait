//! Canonical workspace label request, response, and live update payloads.

use serde::{Deserialize, Serialize};

/// Paseo's fixed workspace label palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceLabelColor {
    /// Violet.
    Violet,
    /// Sky blue.
    Sky,
    /// Emerald green.
    Emerald,
    /// Orange.
    Orange,
    /// Pink.
    Pink,
    /// Indigo.
    Indigo,
    /// Teal.
    Teal,
    /// Red.
    Red,
    /// Amber.
    Amber,
    /// Blue.
    Blue,
}

/// One host-wide label definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelDefinition {
    /// Display name.
    pub name: String,
    /// Palette color.
    pub(crate) color: WorkspaceLabelColor,
}

/// Optional list subscription request. The standalone server assigns the returned ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelSubscribe {
    /// Legacy requested ID accepted by Paseo's schema; modern delivery may replace it.
    #[serde(default)]
    pub(crate) subscription_id: Option<String>,
}

/// Incremental synchronization cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelSyncCursor {
    /// Process generation.
    pub(crate) generation: String,
    /// Last sequence observed by the client.
    pub(crate) after_seq: u64,
}

/// List or subscribe to the host label catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelListRequest {
    /// Subscribe after the coherent initial response.
    #[serde(default)]
    pub(crate) subscribe: Option<WorkspaceLabelSubscribe>,
    /// Optional incremental cursor.
    #[serde(default)]
    pub(crate) sync: Option<WorkspaceLabelSyncCursor>,
}

/// Set one workspace assignment, creating the definition on first assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelAssignmentSetRequest {
    /// Active workspace identity.
    pub(crate) workspace_id: String,
    /// Requested definition.
    pub(crate) label: WorkspaceLabelDefinition,
    /// Whether the label is assigned.
    pub(crate) assigned: bool,
}

/// Edit a definition's name, color, or both in one operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelUpdateRequest {
    /// Existing name, compared case-insensitively after normalization.
    pub(crate) name: String,
    /// Replacement display name.
    #[serde(default)]
    pub(crate) new_name: Option<String>,
    /// Replacement color.
    #[serde(default)]
    pub(crate) color: Option<WorkspaceLabelColor>,
}

/// Delete or inspect a definition by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelDeleteRequest {
    /// Definition name.
    pub(crate) name: String,
}

/// Synchronization response mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceLabelSyncMode {
    /// Complete catalog.
    Snapshot,
    /// Compacted changes after the cursor.
    Changes,
}

/// One removal included in a compacted catch-up response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelRemoval {
    /// Removed display name.
    pub(crate) name: String,
    /// Removal sequence.
    pub(crate) seq: u64,
}

/// Synchronization metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelSyncMetadata {
    /// Snapshot or changes.
    pub(crate) mode: WorkspaceLabelSyncMode,
    /// Current process generation.
    pub(crate) generation: String,
    /// Current sequence.
    pub(crate) head_seq: u64,
    /// Compacted removals.
    pub(crate) removals: Vec<WorkspaceLabelRemoval>,
}

/// Label list response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelListResult {
    /// Server-assigned subscription identity when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) subscription_id: Option<String>,
    /// Full snapshot or compacted upserts.
    pub(crate) labels: Vec<WorkspaceLabelDefinition>,
    /// Synchronization boundary.
    pub(crate) sync: WorkspaceLabelSyncMetadata,
}

/// Assignment response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelAssignmentSetResult {
    /// Authoritative definition.
    pub(crate) label: WorkspaceLabelDefinition,
    /// Complete workspace assignment list.
    pub(crate) workspace_labels: Vec<String>,
}

/// Definition edit response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelUpdateResult {
    /// Updated definition.
    pub(crate) label: WorkspaceLabelDefinition,
    /// Workspaces whose assignment name changed.
    pub(crate) affected_workspace_count: usize,
}

/// Delete inspection and deletion response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceLabelAffectedResult {
    /// Active and archived workspaces carrying the name.
    pub(crate) affected_workspace_count: usize,
}

/// Live catalog update payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum WorkspaceLabelLiveUpdate {
    /// Definition creation or edit.
    Upsert {
        /// Connection-owned subscription identity.
        subscription_id: String,
        /// Current definition.
        label: WorkspaceLabelDefinition,
        /// Previous name for a rename.
        #[serde(skip_serializing_if = "Option::is_none")]
        previous_name: Option<String>,
        /// Process generation.
        generation: String,
        /// Positive sequence.
        seq: u64,
    },
    /// Definition deletion.
    Remove {
        /// Connection-owned subscription identity.
        subscription_id: String,
        /// Deleted display name.
        name: String,
        /// Process generation.
        generation: String,
        /// Positive sequence.
        seq: u64,
    },
}

#[cfg(test)]
mod tests;
