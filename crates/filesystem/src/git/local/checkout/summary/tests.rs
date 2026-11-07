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

#[test]
fn sidebar_porcelain_status_parses_branch_counts_renames_and_untracked_paths() {
    let output = "# branch.oid 0123abcd\0# branch.head feature\0\
        # branch.upstream origin/feature\0# branch.ab +3 -1\0\
        2 R. N... 100644 100644 100644 a b R100 new name.rs\0old name.rs\0\
        ? dir/untracked file.txt\0";
    let status = WorkingStatus::parse(output);
    assert_eq!(status.head.as_deref(), Some("0123abcd"));
    assert_eq!(status.branch.as_deref(), Some("feature"));
    assert_eq!(
        status.upstream,
        Some(AheadBehind {
            ahead: 3,
            behind: 1
        })
    );
    assert!(status.dirty);
    assert_eq!(status.untracked, ["dir/untracked file.txt"]);

    let unborn = WorkingStatus::parse("# branch.oid (initial)\0# branch.head (detached)\0");
    assert_eq!(unborn, WorkingStatus::default());
}

/// The sidebar must agree with the full status read on every field both report.
fn assert_matches_status(repo: &Path, managed: &Path) {
    let checkout = LocalCheckout::new(managed.to_path_buf());
    let cwd = repo.to_str().unwrap();
    let summary = checkout.sidebar_summary(cwd).unwrap().git.unwrap();
    let status = checkout.status(cwd).unwrap();
    assert_eq!(summary.current_branch, status.current_branch, "branch");
    assert_eq!(summary.remote_url, status.remote_url, "remote");
    assert_eq!(summary.is_managed_worktree, status.is_managed_worktree);
    assert_eq!(summary.is_dirty, status.is_dirty, "dirty");
    assert_eq!(
        summary.ahead_behind,
        status
            .ahead_behind
            .map(|counts| (counts.ahead, counts.behind)),
        "default-branch counts"
    );
    assert_eq!(summary.ahead_of_origin, status.ahead_of_origin, "ahead");
    assert_eq!(summary.behind_of_origin, status.behind_of_origin, "behind");
}

#[test]
fn sidebar_summary_matches_full_status_across_remote_and_branch_layouts() {
    let repo = repository();
    let managed = repo.path().join("managed");
    assert_matches_status(repo.path(), &managed);

    git(
        repo.path(),
        &["remote", "add", "zeta", "https://example.com/zeta.git"],
    );
    git(repo.path(), &["remote", "add", "origin", "gh:owner/repo"]);
    git(
        repo.path(),
        &["config", "url.https://github.com/.insteadOf", "gh:"],
    );
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", "HEAD"],
    );
    // Local main moves ahead, so comparing against it instead of origin/main changes counts.
    git(
        repo.path(),
        &["commit", "--allow-empty", "-m", "local main only"],
    );
    git(repo.path(), &["checkout", "-b", "feature"]);
    fs::write(repo.path().join("tracked.txt"), "one\n").unwrap();
    git(repo.path(), &["commit", "-am", "feature"]);
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/feature", "HEAD~1"],
    );
    git(repo.path(), &["branch", "--set-upstream-to=origin/feature"]);
    fs::write(repo.path().join("untracked.txt"), "new\n").unwrap();
    assert_matches_status(repo.path(), &managed);

    // origin/HEAD names the default branch; a dangling target must still be honoured.
    git(
        repo.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    assert_matches_status(repo.path(), &managed);
    git(
        repo.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    );
    assert_matches_status(repo.path(), &managed);

    git(repo.path(), &["checkout", "--detach"]);
    assert_matches_status(repo.path(), &managed);
}

#[test]
fn sidebar_summary_reports_managed_worktree_directories() {
    let repo = repository();
    let managed = tempfile::tempdir().unwrap();
    let worktree = managed.path().join("project").join("linked");
    git(
        repo.path(),
        &[
            "worktree",
            "add",
            "-b",
            "linked",
            worktree.to_str().unwrap(),
        ],
    );
    assert_matches_status(&worktree, managed.path());
    let summary = LocalCheckout::new(managed.path().to_path_buf())
        .sidebar_summary(worktree.to_str().unwrap())
        .unwrap();
    assert!(summary.git.unwrap().is_managed_worktree);
    let dirs = summary.git_dirs.unwrap();
    let common = repo.path().join(".git").canonicalize().unwrap();
    assert_eq!(dirs.common_dir, common);
    assert!(dirs.git_dir.starts_with(common.join("worktrees")));
}

#[test]
fn sidebar_summary_from_a_subdirectory_counts_untracked_files_below_it() {
    let repo = repository();
    let sub = repo.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("inner.txt"), "a\nb\n").unwrap();
    fs::write(repo.path().join("outer.txt"), "outside\n").unwrap();
    fs::write(repo.path().join("tracked.txt"), "one\ntwo\nthree\n").unwrap();

    // Tracked changes cover the repository; untracked files only the workspace directory.
    assert_eq!(
        summary(&sub).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 3,
            deletions: 0
        })
    );
    assert_eq!(
        summary(repo.path()).diff_stat,
        Some(WorkspaceDiffStat {
            additions: 4,
            deletions: 0
        })
    );
}
