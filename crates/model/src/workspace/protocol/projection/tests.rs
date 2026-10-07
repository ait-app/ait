use super::*;
use serde_json::json;

fn workspace(kind: PersistedWorkspaceKind) -> PersistedWorkspaceRecord {
    serde_json::from_value(json!({
        "workspaceId":"wks_test", "projectId":"prj_test", "cwd":"/worktrees/feature/nested",
        "kind":kind, "displayName":"feature", "title":"Custom workspace", "createdAt":"created",
        "updatedAt":"updated", "archivedAt":null, "isPaseoOwnedWorktree":true,
        "worktreeRoot":"/worktrees/feature", "mainRepoRoot":"/main", "labels":["review"]
    }))
    .unwrap()
}

fn project(kind: PersistedProjectKind) -> PersistedProjectRecord {
    serde_json::from_value(json!({
        "projectId":"prj_test", "rootPath":"/main", "kind":kind, "displayName":"main",
        "customName":"Custom project", "customIconRevision":"icon", "projectKey":"remote:host/repo",
        "createdAt":"created", "updatedAt":"updated", "archivedAt":null
    }))
    .unwrap()
}

#[test]
fn descriptors_preserve_custom_fields_and_managed_worktree_placement() {
    let project = project(PersistedProjectKind::Git);
    let descriptor = project_descriptor(&project);
    assert_eq!(descriptor.project_display_name, "Custom project");
    assert_eq!(
        descriptor.project_custom_icon_revision.as_deref(),
        Some("icon")
    );
    assert_eq!(descriptor.project_key, project.project_key);
    assert_eq!(descriptor.project_kind, ProjectKind::Git);
    let descriptor =
        workspace_descriptor(&workspace(PersistedWorkspaceKind::Worktree), Some(&project));
    assert_eq!(descriptor.project_display_name, "Custom project");
    assert_eq!(descriptor.name, "Custom workspace");
    assert_eq!(descriptor.worktree_slug.as_deref(), Some("feature"));
    assert_eq!(descriptor.initial_branch.as_deref(), Some("feature"));
    assert_eq!(descriptor.project_root_path, "/main");
    assert_eq!(descriptor.labels, Some(vec!["review".to_owned()]));
    assert_eq!(descriptor.status, WorkspaceStateBucket::Done);
    assert_eq!(descriptor.status_entered_at.as_deref(), Some("created"));
    assert!(descriptor.git_runtime.is_none());
}

#[test]
fn missing_projects_and_unmanaged_directories_keep_existing_fallbacks() {
    for (kind, wire_kind, project_kind) in [
        (
            PersistedWorkspaceKind::Directory,
            WorkspaceKind::Directory,
            ProjectKind::NonGit,
        ),
        (
            PersistedWorkspaceKind::LocalCheckout,
            WorkspaceKind::LocalCheckout,
            ProjectKind::Git,
        ),
        (
            PersistedWorkspaceKind::Worktree,
            WorkspaceKind::Worktree,
            ProjectKind::Git,
        ),
    ] {
        let mut workspace = workspace(kind);
        workspace.is_paseo_owned_worktree = false;
        workspace.title = None;
        workspace.main_repo_root = None;
        workspace.labels = Some(Vec::new());
        let descriptor = workspace_descriptor(&workspace, None);
        assert_eq!(descriptor.project_display_name, "prj_test");
        assert_eq!(descriptor.project_root_path, workspace.cwd);
        assert_eq!(descriptor.project_kind, project_kind);
        assert_eq!(descriptor.workspace_kind, wire_kind);
        assert_eq!(descriptor.name, "feature");
        assert!(descriptor.worktree_slug.is_none());
        assert!(descriptor.initial_branch.is_none());
        assert!(descriptor.labels.is_none());
    }
    assert_eq!(
        project_descriptor(&project(PersistedProjectKind::NonGit)).project_kind,
        ProjectKind::NonGit
    );
}
