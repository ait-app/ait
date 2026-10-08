use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::git::local::checkout::LocalCheckout;
use crate::git::ports::checkout::{AheadBehind, CheckoutRuntime};

use super::read;

fn git(cwd: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn repository() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.email", "test@example.test"]);
    git(root.path(), &["config", "user.name", "Test"]);
    git(root.path(), &["commit", "--allow-empty", "-m", "base"]);
    root
}

#[test]
fn same_named_remote_counts_are_independent_of_configured_upstream() {
    let repo = repository();
    git(
        repo.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.invalid/repo.git",
        ],
    );
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(repo.path(), &["checkout", "-b", "feature"]);
    git(repo.path(), &["branch", "--set-upstream-to=origin/main"]);
    git(repo.path(), &["commit", "--allow-empty", "-m", "feature"]);
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/feature", "HEAD"],
    );
    let checkout = LocalCheckout::new(repo.path().join("managed"));

    let status = checkout.status(repo.path().to_str().unwrap()).unwrap();

    assert_eq!(status.ahead_of_origin, Some(1));
    let branch = status.branch_status.unwrap();
    assert_eq!(
        branch.remote_ref.as_deref(),
        Some("refs/remotes/origin/feature")
    );
    assert_eq!(
        branch.ahead_behind,
        Some(AheadBehind {
            ahead: 0,
            behind: 0
        })
    );
    assert_eq!(
        branch.head_sha,
        Some(git(repo.path(), &["rev-parse", "HEAD"]))
    );
}

#[test]
fn absent_or_deleted_remote_branches_keep_counts_unknown_and_preserve_head() {
    let repo = repository();
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    for remote in [None, Some("origin")] {
        let status = read(repo.path(), Some("main"), remote, "").unwrap();
        assert_eq!(status.head_sha.as_deref(), Some(head.as_str()));
        assert_eq!(status.remote_ref, None);
        assert_eq!(status.ahead_behind, None);
    }
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    git(
        repo.path(),
        &["update-ref", "-d", "refs/remotes/origin/main"],
    );
    let status = read(repo.path(), Some("main"), Some("origin"), "").unwrap();
    assert_eq!(status.remote_ref, None);
    assert_eq!(status.head_sha.as_deref(), Some(head.as_str()));
}

#[test]
fn conflict_facts_cover_every_unmerged_index_state_and_ignore_normal_changes() {
    let repo = repository();
    for code in ["DD", "AU", "UD", "UA", "DU", "AA", "UU"] {
        let status = read(repo.path(), Some("main"), None, &format!("{code} file")).unwrap();
        assert!(status.has_conflicts, "{code}");
    }
    let status = read(
        repo.path(),
        Some("main"),
        None,
        " M changed\nA  added\n?? new",
    )
    .unwrap();
    assert!(!status.has_conflicts);
}

#[test]
fn unborn_and_detached_checkouts_do_not_fabricate_remote_counts() {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    let status = read(repo.path(), Some("main"), None, "").unwrap();
    assert!(status.head_sha.is_none());
    let repo = repository();
    let status = read(repo.path(), None, Some("origin"), "").unwrap();
    assert!(status.head_sha.is_some());
    assert!(status.remote_ref.is_none());
}

#[test]
fn same_named_remote_counts_distinguish_ahead_behind_and_diverged() {
    let repo = repository();
    let base = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["commit", "--allow-empty", "-m", "local"]);
    let local = git(repo.path(), &["rev-parse", "HEAD"]);
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", &base],
    );
    assert_eq!(
        read(repo.path(), Some("main"), Some("origin"), "")
            .unwrap()
            .ahead_behind,
        Some(AheadBehind {
            ahead: 1,
            behind: 0
        })
    );
    git(repo.path(), &["commit", "--allow-empty", "-m", "remote"]);
    let incoming = git(repo.path(), &["rev-parse", "HEAD"]);
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", &incoming],
    );
    git(repo.path(), &["reset", "--hard", &local]);
    assert_eq!(
        read(repo.path(), Some("main"), Some("origin"), "")
            .unwrap()
            .ahead_behind,
        Some(AheadBehind {
            ahead: 0,
            behind: 1
        })
    );
    git(repo.path(), &["commit", "--allow-empty", "-m", "diverged"]);
    assert_eq!(
        read(repo.path(), Some("main"), Some("origin"), "")
            .unwrap()
            .ahead_behind,
        Some(AheadBehind {
            ahead: 1,
            behind: 1
        })
    );
}
