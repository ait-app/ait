//! Exact Workspace lookups for the first screen, with cached runtime and stable activity.

use domain::workspace::protocol::projection::{project_descriptor, workspace_descriptor};
use domain::workspace::records::PersistedProjectRecord;
use serde_json::Value;

use crate::directory::protocol::directory::{
    WorkspaceGetRequest, WorkspaceGetResult, WorkspaceGetState,
};
use crate::support::rpc::encode;
use crate::support::time::now_timestamp;

use super::{Directory, ErrorCode, directory_error, runtime};

/// Project the exact `request` target from `directory` without creating or restoring records.
/// Returns safe validation, registry, activity-source or encoding errors.
pub(super) fn get(directory: &Directory, request: WorkspaceGetRequest) -> Result<Value, ErrorCode> {
    if request.workspace_id.trim().is_empty() || request.workspace_id.len() > 256 {
        return Err(ErrorCode::InvalidMessage);
    }
    let mut result = WorkspaceGetResult {
        workspace_id: request.workspace_id,
        state: WorkspaceGetState::Missing,
        workspace: None,
        project: None,
    };
    let Some((workspace, project)) = directory
        .workspace_target(&result.workspace_id)
        .map_err(directory_error)?
    else {
        return encode(result, ErrorCode::RegistryIo);
    };
    if workspace.is_archived()
        || project
            .as_ref()
            .is_some_and(PersistedProjectRecord::is_archived)
    {
        result.state = WorkspaceGetState::Archived;
        return encode(result, ErrorCode::RegistryIo);
    }
    let mut descriptor = workspace_descriptor(&workspace, project.as_ref());
    if let Some(snapshot) = directory.runtime_snapshot(&workspace.cwd) {
        runtime::apply(&mut descriptor, &snapshot);
    }
    let status = directory
        .workspace_status(&workspace, &now_timestamp())
        .map_err(directory_error)?;
    descriptor.status = status.bucket;
    descriptor.status_entered_at = Some(status.entered_at);
    result.state = WorkspaceGetState::Ready;
    result.workspace = Some(descriptor);
    result.project = project.as_ref().map(project_descriptor);
    encode(result, ErrorCode::RegistryIo)
}
