use super::{
    DirectoryError, WorkspaceListRequest, directory_create_error_code, matches_filter,
    workspace_create_error_code,
};
use domain::workspace::protocol::projection::workspace_descriptor;
use domain::workspace::records::{PersistedWorkspaceKind, PersistedWorkspaceRecord};
use serde_json::json;

#[test]
fn directory_creation_failures_keep_the_documented_business_error_codes() {
    for (error, code) in [
        (
            DirectoryError::DirectoryNotFound,
            Some("directory_not_found"),
        ),
        (DirectoryError::UnknownProject, Some("unknown_project")),
        (DirectoryError::ArchivedProject, Some("archived_project")),
        (
            DirectoryError::WorkspaceIdConflict,
            Some("workspace_id_conflict"),
        ),
        (DirectoryError::ProjectIconStorage, None),
    ] {
        assert_eq!(workspace_create_error_code(&error), code);
    }
    assert_eq!(
        directory_create_error_code(&DirectoryError::DirectoryNotFound),
        "parent_directory_not_found"
    );
    assert_eq!(
        directory_create_error_code(&DirectoryError::ArchivedProject),
        "registration_failed"
    );
    assert_eq!(
        directory_create_error_code(&DirectoryError::ProjectIconStorage),
        "filesystem_error"
    );
}

#[test]
fn managed_descriptor_keeps_initial_branch_after_title_and_branch_change() {
    let mut workspace: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId": "workspace-1",
        "projectId": "project-1",
        "cwd": "/worktrees/initial-workspace",
        "kind": "worktree",
        "displayName": "initial-workspace",
        "title": "New title",
        "branch": "renamed-workspace",
        "worktreeRoot": "/worktrees/initial-workspace",
        "isPaseoOwnedWorktree": true,
        "createdAt": "2026-10-05T00:00:00Z",
        "updatedAt": "2026-10-05T00:00:00Z",
        "archivedAt": null
    }))
    .expect("workspace record");

    let managed = workspace_descriptor(&workspace, None);
    assert_eq!(managed.name, "New title");
    assert_eq!(managed.initial_branch.as_deref(), Some("initial-workspace"));
    let foreign_project: WorkspaceListRequest =
        serde_json::from_value(json!({"filter":{"projectId":"other-project"}})).unwrap();
    assert!(!matches_filter(&workspace, &foreign_project));

    workspace.kind = PersistedWorkspaceKind::LocalCheckout;
    workspace.is_paseo_owned_worktree = false;
    let external = workspace_descriptor(&workspace, None);
    assert_eq!(external.initial_branch, None);
}
