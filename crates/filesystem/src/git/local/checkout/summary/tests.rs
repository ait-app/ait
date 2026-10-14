//! Local Git fixtures for the lightweight sidebar line counts.

use std::path::Path;
use std::process::Command;

use super::*;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    fs::write(repo.path().join("tracked.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "base"]);
    repo
}

fn summary(repo: &Path) -> WorkspaceGitSnapshot {
    LocalCheckout::new(repo.join("managed"))
        .sidebar_summary(repo.to_str().unwrap())
        .unwrap()
        .git
        .unwrap()
}

#[test]
fn sidebar_summary_includes_committed_staged_working_and_untracked_changes() {
    let repo = repository();
    git(repo.path(), &["checkout", "-b", "feature"]);
    fs::write(repo.path().join("tracked.txt"), "one\ntwo\nthree\n").unwrap();
    git(repo.path(), &["commit", "-am", "feature"]);
    fs::write(repo.path().join("tracked.txt"), "one\nthree\nfour\n").unwrap();
    git(repo.path(), &["add", "."]);
    fs::write(repo.path().join("untracked.txt"), "new\nlast").unwrap();
    fs::write(repo.path().join("binary.dat"), b"binary\0data").unwrap();

    let result = summary(repo.path());
    assert_eq!(
        result.diff_stat,
        Some(WorkspaceDiffStat {
            additions: 4,
            deletions: 1
        })
    );
    assert_eq!(result.current_branch.as_deref(), Some("feature"));
    assert_eq!(result.is_dirty, Some(true));
    git(repo.path(), &["reset", "--hard", "HEAD"]);
    git(repo.path(), &["clean", "-fd"]);
    assert_eq!(
        summary(repo.path()).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 1,
            deletions: 0
        })
    );
}

#[test]
fn sidebar_summary_clears_merged_changes_when_origin_main_moves() {
    use crate::git::ports::checkout::{CheckoutDiffCompare, CheckoutDiffMode};

    let repo = repository();
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "main"],
    );
    git(repo.path(), &["checkout", "-b", "feature"]);
    fs::write(repo.path().join("tracked.txt"), "one\nthree\n").unwrap();
    git(repo.path(), &["commit", "-am", "feature"]);
    assert_eq!(
        summary(repo.path()).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 1,
            deletions: 1
        })
    );

    // A fetch after merging moves origin/main while the local main and feature HEAD stay put.
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    let checkout = LocalCheckout::new(repo.path().join("managed"));
    let diff = checkout
        .diff(
            repo.path().to_str().unwrap(),
            &CheckoutDiffCompare {
                mode: CheckoutDiffMode::Base,
                base_ref: Some("main".to_owned()),
                ignore_whitespace: false,
            },
        )
        .unwrap();
    assert!(diff.files.is_empty());
    let merged = summary(repo.path());
    assert_eq!(merged.is_dirty, Some(false));
    assert!(merged.diff_stat.is_none());

    fs::write(repo.path().join("tracked.txt"), "one\nthree\nfour\n").unwrap();
    fs::write(repo.path().join("untracked.txt"), "new\n").unwrap();
    assert_eq!(
        summary(repo.path()).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 2,
            deletions: 0
        })
    );
}

#[test]
fn sidebar_summary_main_compares_against_origin_and_clean_local_repos_hide_counts() {
    let repo = repository();
    assert!(summary(repo.path()).diff_stat.is_none());
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    fs::write(repo.path().join("tracked.txt"), "one\n").unwrap();
    git(repo.path(), &["commit", "-am", "main change"]);
    assert_eq!(
        summary(repo.path()).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 0,
            deletions: 1
        })
    );
}

#[test]
fn sidebar_summary_supports_unborn_detached_and_non_git_directories() {
    let plain = tempfile::tempdir().unwrap();
    let checkout = LocalCheckout::new(plain.path().join("managed"));
    assert!(
        checkout
            .sidebar_summary(plain.path().to_str().unwrap())
            .unwrap()
            .git
            .is_none()
    );
    git(plain.path(), &["init", "-b", "main"]);
    fs::write(plain.path().join("new.txt"), "first\n").unwrap();
    git(plain.path(), &["add", "."]);
    assert_eq!(summary(plain.path()).diff_stat.unwrap().additions, 1);
    git(plain.path(), &["commit", "-m", "initial"]);
    git(plain.path(), &["checkout", "--detach"]);
    fs::write(plain.path().join("new.txt"), "changed\n").unwrap();
    let detached = summary(plain.path());
    assert!(detached.current_branch.is_none());
    assert_eq!(
        detached.diff_stat,
        Some(WorkspaceDiffStat {
            additions: 1,
            deletions: 1
        })
    );
}

#[test]
fn sidebar_numstat_handles_renames_binary_and_paths_containing_delimiters() {
    let value = "1\t2\t\0old\tfile\n.rs\0new.rs\0-\t-\tbinary\0";
    assert_eq!(
        parse_numstat(value),
        WorkspaceDiffStat {
            additions: 1,
            deletions: 2
        }
    );
}

#[test]
fn sidebar_untracked_statistics_fail_closed_when_the_read_budget_is_exhausted() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("text");
    fs::write(&file, "hello\n").unwrap();
    assert!(count_lines(&file, &mut 3).is_err());
    fs::write(&file, "").unwrap();
    assert_eq!(count_lines(&file, &mut 0).unwrap(), 0);
}
