use domain::workspace::protocol::projection::workspace_descriptor;
use domain::workspace::records::{PersistedWorkspaceKind, PersistedWorkspaceRecord};
use serde_json::json;

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

    workspace.kind = PersistedWorkspaceKind::LocalCheckout;
    workspace.is_paseo_owned_worktree = false;
    let external = workspace_descriptor(&workspace, None);
    assert_eq!(external.initial_branch, None);
}
