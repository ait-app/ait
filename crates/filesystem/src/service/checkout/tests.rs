use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use metadata::model::registry::PersistedWorkspaceRecord;
use metadata::ports::registry::{WorkspaceMutationContext, WorkspaceRegistry};
use metadata::storage::registry::FileBackedWorkspaceRegistry;
use serde_json::json;

use super::Checkout;
use crate::local::checkout::LocalCheckout;
use crate::ports::checkout::CheckoutFailureKind;

#[test]
fn reset_workspace_validates_and_updates_the_durable_branch() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let repo = temp.path().join("repository");
    std::fs::create_dir(&repo).expect("repository directory");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.name", "Server Test"]);
    git(&repo, &["config", "user.email", "server@example.invalid"]);
    std::fs::write(repo.join("tracked.txt"), "baseline\n").expect("baseline file");
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-m", "baseline"]);
    let remote = temp.path().join("remote.git");
    git(
        temp.path(),
        &["init", "--bare", "-b", "master", remote.to_str().unwrap()],
    );
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&repo, &["push", "origin", "main:master"]);
    let managed = temp.path().join("managed");
    let linked = managed.join("repository-hash/initial-workspace");
    std::fs::create_dir_all(linked.parent().unwrap()).expect("managed parent");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "initial-workspace",
            linked.to_str().unwrap(),
        ],
    );
    git(&linked, &["branch", "-m", "renamed-workspace"]);

    let registry = FileBackedWorkspaceRegistry::new(temp.path().join("workspaces.json"));
    let workspace: PersistedWorkspaceRecord = serde_json::from_value(json!({
        "workspaceId": "workspace-1",
        "projectId": "project-1",
        "cwd": linked.to_str().unwrap(),
        "kind": "worktree",
        "displayName": "initial-workspace",
        "branch": "renamed-workspace",
        "worktreeRoot": linked.to_str().unwrap(),
        "isPaseoOwnedWorktree": true,
        "createdAt": "2026-10-05T00:00:00Z",
        "updatedAt": "2026-10-05T00:00:00Z",
        "archivedAt": null
    }))
    .expect("workspace record");
    registry
        .upsert(&workspace, WorkspaceMutationContext::default())
        .expect("persist workspace");
    let checkout = Checkout::new(Box::new(LocalCheckout::new(managed)))
        .with_workspace_registry(Arc::new(registry.clone()));

    let error = checkout
        .reset_workspace(linked.to_str().unwrap(), "workspace-1", "wrong-name")
        .unwrap_err();
    assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
    assert_eq!(branch(&linked), "renamed-workspace");

    checkout
        .reset_workspace(linked.to_str().unwrap(), "workspace-1", "initial-workspace")
        .expect("reset workspace");
    assert_eq!(branch(&linked), "initial-workspace");
    assert_eq!(
        registry
            .get("workspace-1")
            .unwrap()
            .unwrap()
            .branch
            .as_deref(),
        Some("initial-workspace")
    );
}

fn git(cwd: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?}: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn branch(cwd: &Path) -> String {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .expect("read branch");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
