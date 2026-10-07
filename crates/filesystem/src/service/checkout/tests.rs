use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use file::storage::registry::FileBackedWorkspaceRegistry;
use model::workspace::records::PersistedWorkspaceRecord;
use model::workspace::registry::{WorkspaceMutationContext, WorkspaceRegistry};
use serde_json::json;
use tempfile::TempDir;

use super::Checkout;
use crate::local::checkout::LocalCheckout;
use crate::ports::checkout::CheckoutFailureKind;

#[test]
fn reset_workspace_validates_and_updates_the_durable_branch() {
    let fixture = ResetFixture::new("");

    let error = fixture
        .checkout
        .reset_workspace(
            fixture.linked.to_str().unwrap(),
            "workspace-1",
            "wrong-name",
        )
        .unwrap_err();
    assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
    assert_eq!(branch(&fixture.linked), "renamed-workspace");

    fixture
        .checkout
        .reset_workspace(
            fixture.linked.to_str().unwrap(),
            "workspace-1",
            "initial-workspace",
        )
        .expect("reset workspace");
    fixture.assert_reset();
}

#[test]
fn reset_workspace_accepts_equivalent_directory_spellings() {
    for (registered_suffix, requested_suffix) in [("/", ""), ("", "/"), ("//", ""), ("/.", "")] {
        let fixture = ResetFixture::new(registered_suffix);
        let registered_cwd = fixture.workspace().cwd;
        let requested_cwd = format!("{}{requested_suffix}", fixture.linked.display());
        std::fs::write(fixture.linked.join("tracked.txt"), "local changes\n")
            .expect("dirty tracked file");

        fixture
            .checkout
            .reset_workspace(&requested_cwd, "workspace-1", "initial-workspace")
            .expect("reset equivalent workspace path");

        fixture.assert_reset();
        assert_eq!(fixture.workspace().cwd, registered_cwd);
        assert_eq!(
            std::fs::read_to_string(fixture.linked.join("tracked.txt")).unwrap(),
            "baseline\n"
        );
    }
}

#[test]
fn reset_workspace_rejects_different_directories_without_mutation() {
    let fixture = ResetFixture::new("/");
    let nested = fixture.linked.join("nested");
    std::fs::create_dir(&nested).expect("nested workspace directory");
    let sibling = fixture.linked.with_file_name("other-workspace");
    git(
        &fixture.linked,
        &[
            "worktree",
            "add",
            "-b",
            "other-workspace",
            sibling.to_str().unwrap(),
        ],
    );
    let original = fixture.workspace();

    for requested_cwd in [&nested, &sibling] {
        let error = fixture
            .checkout
            .reset_workspace(
                requested_cwd.to_str().unwrap(),
                "workspace-1",
                "initial-workspace",
            )
            .unwrap_err();

        assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
        assert_eq!(branch(&fixture.linked), "renamed-workspace");
        assert_eq!(branch(&sibling), "other-workspace");
        assert_eq!(fixture.workspace(), original);
    }
}

#[test]
fn reset_workspace_rejects_archived_and_unmanaged_records_without_mutation() {
    for (archived, managed) in [(true, true), (false, false)] {
        let fixture = ResetFixture::new("/");
        let mut original = fixture.workspace();
        original.archived_at = archived.then(|| "2026-10-05T00:00:00Z".to_owned());
        original.is_paseo_owned_worktree = managed;
        fixture
            .registry
            .upsert(&original, WorkspaceMutationContext::default())
            .expect("update workspace eligibility");

        let error = fixture
            .checkout
            .reset_workspace(
                fixture.linked.to_str().unwrap(),
                "workspace-1",
                "initial-workspace",
            )
            .unwrap_err();

        assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
        assert_eq!(branch(&fixture.linked), "renamed-workspace");
        assert_eq!(fixture.workspace(), original);
    }
}

struct ResetFixture {
    _temp: TempDir,
    linked: PathBuf,
    registry: FileBackedWorkspaceRegistry,
    checkout: Checkout,
}

impl ResetFixture {
    fn new(registered_cwd_suffix: &str) -> Self {
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
            "cwd": format!("{}{registered_cwd_suffix}", linked.display()),
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
        Self {
            _temp: temp,
            linked,
            registry,
            checkout,
        }
    }

    fn workspace(&self) -> PersistedWorkspaceRecord {
        self.registry
            .get("workspace-1")
            .expect("read workspace registry")
            .expect("persisted workspace")
    }

    fn assert_reset(&self) {
        assert_eq!(branch(&self.linked), "initial-workspace");
        assert_eq!(
            self.workspace().branch.as_deref(),
            Some("initial-workspace")
        );
    }
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
