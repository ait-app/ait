//! Bounded identity-based Workspace reads, independent of the complete sidebar directory.

use domain::workspace::protocol::workspace::{
    WorkspaceDescriptorPayload, WorkspaceProjectDescriptorPayload,
};
use serde::{Deserialize, Serialize};

/// Read one persisted Workspace by identity without listing its host directory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct WorkspaceGetRequest {
    /// Exact persisted Workspace identity, never a path or a prefix.
    pub(crate) workspace_id: String,
}

/// Target availability, including archived records hidden from the sidebar.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum WorkspaceGetState {
    /// The active descriptor is available.
    Ready,
    /// The Workspace or its Project is archived.
    Archived,
    /// The identity does not exist.
    Missing,
}

/// A target result; descriptors are returned only for an active Workspace.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorkspaceGetResult {
    /// Requested identity, also present for missing targets.
    pub(crate) workspace_id: String,
    /// Availability of the target.
    pub(crate) state: WorkspaceGetState,
    /// Active target descriptor with cached runtime and activity projection.
    pub(crate) workspace: Option<WorkspaceDescriptorPayload>,
    /// Owning Project descriptor, when the record exists.
    pub(crate) project: Option<WorkspaceProjectDescriptorPayload>,
}
