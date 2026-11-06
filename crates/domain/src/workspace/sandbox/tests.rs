use std::path::{Path, PathBuf};

use super::*;

fn record(
    sandbox: Option<WorkspaceSandbox>,
    worktree_root: Option<&str>,
) -> PersistedWorkspaceRecord {
    serde_json::from_value(serde_json::json!({
        "workspaceId": "wks_0123456789abcdef",
        "projectId": "prj_1",
        "cwd": "/repo/worktrees/feature/sub",
        "kind": "worktree",
        "displayName": "feature",
        "worktreeRoot": worktree_root,
        "createdAt": "2026-01-01T00:00:00Z",
        "updatedAt": "2026-01-01T00:00:00Z",
        "archivedAt": null,
        "sandbox": sandbox,
    }))
    .expect("valid workspace record")
}

#[test]
fn sandbox_level_uses_snake_case_wire_value() {
    assert_eq!(
        serde_json::to_value(WorkspaceSandbox::WorktreeWrites).expect("serializable"),
        "worktree_writes"
    );
}

#[test]
fn unsandboxed_workspace_needs_no_confinement() {
    assert_eq!(
        WriteConfinement::for_workspace(&record(None, Some("/repo/worktrees/feature"))),
        None
    );
}

#[test]
fn confinement_prefers_recorded_worktree_root() {
    let workspace = record(
        Some(WorkspaceSandbox::WorktreeWrites),
        Some("/repo/worktrees/feature"),
    );

    let confinement = WriteConfinement::for_workspace(&workspace).expect("sandboxed");

    assert_eq!(
        confinement.worktree_root(),
        Path::new("/repo/worktrees/feature")
    );
}

#[test]
fn confinement_falls_back_to_selected_directory() {
    let workspace = record(Some(WorkspaceSandbox::WorktreeWrites), None);

    let confinement = WriteConfinement::for_workspace(&workspace).expect("sandboxed");

    assert_eq!(
        confinement.worktree_root(),
        Path::new("/repo/worktrees/feature/sub")
    );
}

#[test]
fn linked_worktree_policy_allows_common_dir_and_protects_executable_git_state() {
    let policy = WritePolicy::worktree(
        Path::new("/wt"),
        Path::new("/repo/.git/worktrees/wt"),
        Path::new("/repo/.git"),
    );

    assert_eq!(
        policy.trees(),
        [PathBuf::from("/wt"), PathBuf::from("/repo/.git")]
    );
    assert_eq!(
        policy.protected(),
        [
            PathBuf::from("/wt/.git"),
            PathBuf::from("/repo/.git/config"),
            PathBuf::from("/repo/.git/hooks"),
            PathBuf::from("/repo/.git/worktrees/wt/config.worktree"),
        ]
    );
    assert!(policy.prefixes().is_empty());
}

#[test]
fn separate_git_dir_is_allowed_alongside_common_dir() {
    let policy = WritePolicy::worktree(
        Path::new("/wt"),
        Path::new("/elsewhere/wt.git"),
        Path::new("/repo/.git"),
    );

    assert_eq!(
        policy.trees(),
        [
            PathBuf::from("/wt"),
            PathBuf::from("/repo/.git"),
            PathBuf::from("/elsewhere/wt.git"),
        ]
    );
}

#[test]
fn added_locations_are_reported_in_insertion_order() {
    let mut policy = WritePolicy::default();

    policy.allow_tree(PathBuf::from("/state"));
    policy.allow_prefix(PathBuf::from("/home/.state.json"));
    policy.protect(PathBuf::from("/state/config.toml"));

    assert_eq!(policy.trees(), [PathBuf::from("/state")]);
    assert_eq!(policy.prefixes(), [PathBuf::from("/home/.state.json")]);
    assert_eq!(policy.protected(), [PathBuf::from("/state/config.toml")]);
}
