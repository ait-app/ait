use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CheckoutFailureKind, CheckoutRuntime, Fixture, LocalCheckout, git, git_output};
use crate::git::ports::checkout::AheadBehind;

const CONFLICT_MESSAGE: &str = "Rebase hit conflicts and was undone";
const DUPLICATE_MESSAGE: &str = "Rebasing would duplicate commits that are already published, so this branch cannot be rebased here";
const DIFFERENT_DESTINATION_MESSAGE: &str =
    "This branch is pushed to a different remote branch, so it cannot be rebased here";

#[test]
fn rebase_replays_local_commits_onto_the_remote_branch() {
    let fixture = RebaseFixture::new("origin", "main");
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");
    commit_file(
        &fixture.repository.repo,
        "local.txt",
        "local\n",
        "local change",
    );

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    let branch_status = fixture
        .runtime
        .status(fixture.cwd())
        .expect("status")
        .branch_status
        .expect("branch status");
    assert_eq!(
        branch_status.remote_ref.as_deref(),
        Some("refs/remotes/origin/main")
    );
    assert_eq!(
        branch_status.ahead_behind,
        Some(AheadBehind {
            ahead: 1,
            behind: 0
        })
    );
    assert!(fixture.is_ancestor_of_head(&remote_tip));
    assert!(fixture.repository.repo.join("local.txt").exists());
}

#[test]
fn rebase_fast_forwards_a_branch_that_is_only_behind() {
    let fixture = RebaseFixture::new("origin", "main");
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    assert_eq!(fixture.head(), remote_tip);
}

#[test]
fn rebase_keeps_a_local_merge_of_the_base_branch() {
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    // The remote branch already holds feature work, so the base branch's commits fork before
    // the remote branch and `--rebase-merges` keeps them at their original branch point.
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    git(repo, &["push"]);
    git(&fixture.peer, &["pull", "--ff-only"]);
    git(repo, &["checkout", "main"]);
    let base_commit = commit_file(repo, "base.txt", "base\n", "base change");
    git(repo, &["checkout", "feature"]);
    git(repo, &["merge", "--no-ff", "main", "-m", "merge main"]);
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    let parents = git_output(repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3, "{parents}");
    assert_eq!(git_output(repo, &["rev-parse", "HEAD^2"]), base_commit);
    assert_eq!(git_output(repo, &["rev-parse", "main"]), base_commit);
    assert!(fixture.is_ancestor_of_head(&base_commit));
    assert!(fixture.is_ancestor_of_head(&remote_tip));
}

#[test]
fn rebase_keeps_a_published_base_commit_merged_locally() {
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    git(repo, &["push"]);
    git(&fixture.peer, &["pull", "--ff-only"]);
    let base_commit = publish_base_commit(repo);
    git(repo, &["merge", "--no-ff", "main", "-m", "merge main"]);
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    assert_eq!(git_output(repo, &["rev-parse", "HEAD^2"]), base_commit);
    assert!(fixture.is_ancestor_of_head(&remote_tip));
}

#[test]
fn rebase_refuses_to_duplicate_published_commits_from_a_local_merge() {
    // The remote branch was pushed at the fork point, so the base branch's commit is not a
    // cousin and `--rebase-merges` would recreate it on top of the remote branch.
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    publish_base_commit(repo);
    git(repo, &["merge", "--no-ff", "main", "-m", "merge main"]);
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DUPLICATE_MESSAGE, &before);
    fixture.assert_clean_without_rebase();
}

#[test]
fn rebase_refuses_to_duplicate_published_commits_under_an_unpublished_merge_parent() {
    // The merged-in tip is local, but its parent is on `origin/main`, and `--rebase-merges`
    // would recreate both on top of the remote branch.
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    let published_base = publish_base_commit(repo);
    git(repo, &["checkout", "main"]);
    commit_file(repo, "local-base.txt", "local\n", "unpublished base change");
    git(repo, &["checkout", "feature"]);
    git(repo, &["merge", "--no-ff", "main", "-m", "merge main"]);
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DUPLICATE_MESSAGE, &before);
    fixture.assert_clean_without_rebase();
    assert!(fixture.is_ancestor_of_head(&published_base));
}

#[test]
fn rebase_refusal_leaves_other_local_branches_unchanged() {
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    git(repo, &["config", "rebase.updateRefs", "true"]);
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    let base_commit = publish_base_commit(repo);
    git(repo, &["merge", "--no-ff", "main", "-m", "merge main"]);
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DUPLICATE_MESSAGE, &before);
    assert_eq!(git_output(repo, &["rev-parse", "main"]), base_commit);
}

#[test]
fn rebase_replays_a_local_merge_of_an_unpublished_branch() {
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    git(repo, &["checkout", "-b", "side"]);
    commit_file(repo, "side.txt", "side\n", "side change");
    git(repo, &["checkout", "feature"]);
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    git(repo, &["merge", "--no-ff", "side", "-m", "merge side"]);
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    let parents = git_output(repo, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split_whitespace().count(), 3, "{parents}");
    assert!(fixture.is_ancestor_of_head(&remote_tip));
    assert!(repo.join("side.txt").exists());
}

#[test]
fn rebase_conflict_is_undone_and_reported() {
    let fixture = RebaseFixture::new("origin", "main");
    fixture.publish("tracked.txt", "peer\n", "peer change");
    commit_file(
        &fixture.repository.repo,
        "tracked.txt",
        "local\n",
        "local change",
    );
    let before = fixture.head();

    let error = fixture
        .runtime
        .rebase(fixture.cwd())
        .expect_err("conflicting rebase");

    assert_eq!(error.kind, CheckoutFailureKind::MergeConflict);
    assert_eq!(error.message, CONFLICT_MESSAGE);
    assert_eq!(fixture.head(), before);
    fixture.assert_clean_without_rebase();
}

#[test]
fn rebase_refuses_tracked_changes() {
    let fixture = RebaseFixture::new("origin", "main");
    fixture.publish("peer.txt", "peer\n", "peer change");
    let repo = &fixture.repository.repo;
    let before = fixture.head();

    std::fs::write(repo.join("tracked.txt"), "modified\n").expect("modify tracked file");
    fixture.assert_refused("Commit or stash your changes before rebasing", &before);

    git(repo, &["add", "tracked.txt"]);
    fixture.assert_refused("Commit or stash your changes before rebasing", &before);
}

#[test]
fn rebase_refuses_a_detached_head() {
    let fixture = RebaseFixture::new("origin", "main");
    fixture.publish("peer.txt", "peer\n", "peer change");
    git(&fixture.repository.repo, &["checkout", "--detach"]);
    let before = fixture.head();

    let error = fixture
        .runtime
        .rebase(fixture.cwd())
        .expect_err("rebase of a detached HEAD");

    // The current-branch check shared with pull and push reports this as `Unknown`.
    assert_eq!(error.kind, CheckoutFailureKind::Unknown);
    assert_eq!(
        error.message,
        "Unable to determine current branch for rebase"
    );
    assert_eq!(fixture.head(), before);
}

#[test]
fn rebase_refuses_without_a_remote() {
    let fixture = Fixture::new();
    let runtime = LocalCheckout::new(fixture.temp.path().join("managed"));

    let error = runtime
        .rebase(fixture.repo.to_str().expect("UTF-8 repository path"))
        .expect_err("rebase without a remote");

    assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
    assert_eq!(error.message, "No remote is configured");
}

#[test]
fn rebase_passes_other_fetch_failures_through() {
    let fixture = RebaseFixture::new("origin", "main");
    let missing = fixture.repository.temp.path().join("missing.git");
    let missing = missing.to_str().expect("UTF-8 missing remote path");
    git(
        &fixture.repository.repo,
        &["remote", "set-url", "origin", missing],
    );
    let before = fixture.head();

    let error = fixture
        .runtime
        .rebase(fixture.cwd())
        .expect_err("fetch from a missing remote");

    assert_eq!(error.kind, CheckoutFailureKind::Unknown);
    assert!(error.message.contains(missing), "{}", error.message);
    assert_eq!(fixture.head(), before);
}

#[test]
fn rebase_refuses_when_the_remote_branch_is_missing() {
    let fixture = RebaseFixture::new("origin", "main");
    git(&fixture.repository.repo, &["checkout", "-b", "unpublished"]);
    let before = fixture.head();

    fixture.assert_refused("The remote has no branch named unpublished", &before);
}

#[test]
fn rebase_refuses_when_push_goes_to_a_different_branch() {
    let fixture = RebaseFixture::new("origin", "main");
    let repo = &fixture.repository.repo;
    git(repo, &["push", "origin", "main:review"]);
    git(repo, &["branch", "--set-upstream-to", "origin/review"]);
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DIFFERENT_DESTINATION_MESSAGE, &before);
}

#[test]
fn rebase_refuses_when_push_remote_goes_to_a_different_branch() {
    let fixture = RebaseFixture::new("origin", "main");
    let repo = &fixture.repository.repo;
    git(repo, &["config", "branch.main.pushRemote", "origin"]);
    git(
        repo,
        &["config", "remote.origin.push", "HEAD:refs/heads/review"],
    );
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DIFFERENT_DESTINATION_MESSAGE, &before);
}

#[test]
fn rebase_refuses_while_a_cherry_pick_sequence_is_paused_and_leaves_it() {
    let fixture = RebaseFixture::new("origin", "main");
    let repo = &fixture.repository.repo;
    git(repo, &["checkout", "-b", "side"]);
    git(repo, &["commit", "--allow-empty", "-m", "empty change"]);
    commit_file(repo, "side.txt", "side\n", "side change");
    git(repo, &["checkout", "main"]);
    let cherry_pick = Command::new("git")
        .args(["cherry-pick", "side~1", "side"])
        .current_dir(repo)
        .output()
        .expect("run git cherry-pick");
    assert!(
        !cherry_pick.status.success(),
        "the sequence must stop on the empty commit"
    );
    // Committing the stopped pick leaves only the sequencer state and a clean tree.
    git(
        repo,
        &["commit", "--allow-empty", "-m", "kept empty change"],
    );
    assert!(!fixture.git_path_exists("CHERRY_PICK_HEAD"));
    let before = fixture.head();

    fixture.assert_refused(
        "Finish or abort the operation in progress before rebasing",
        &before,
    );
    assert!(fixture.git_path_exists("sequencer"));
}

#[test]
fn rebase_refuses_while_another_operation_is_in_progress_and_leaves_it() {
    let fixture = RebaseFixture::new("origin", "main");
    let repo = &fixture.repository.repo;
    git(repo, &["checkout", "-b", "side"]);
    commit_file(repo, "tracked.txt", "side\n", "side change");
    git(repo, &["checkout", "main"]);
    commit_file(repo, "tracked.txt", "main\n", "main change");
    let merge = Command::new("git")
        .args(["merge", "side"])
        .current_dir(repo)
        .output()
        .expect("run git merge");
    assert!(!merge.status.success(), "the merge must stop on a conflict");
    let before = fixture.head();

    fixture.assert_refused(
        "Finish or abort the operation in progress before rebasing",
        &before,
    );
    assert!(fixture.git_path_exists("MERGE_HEAD"));
}

#[test]
fn rebase_refuses_to_duplicate_published_base_commits_after_a_local_rebase() {
    // The pushed branch was rebased locally onto a base that gained published commits; replaying
    // it onto its remote copy would recreate those base commits under new IDs.
    let fixture = RebaseFixture::new("origin", "feature");
    let repo = &fixture.repository.repo;
    commit_file(repo, "feature.txt", "feature\n", "feature change");
    git(repo, &["push"]);
    git(repo, &["checkout", "main"]);
    commit_file(repo, "base-1.txt", "base\n", "first base change");
    commit_file(repo, "base-2.txt", "base\n", "second base change");
    git(repo, &["push", "origin", "main"]);
    git(repo, &["checkout", "feature"]);
    git(repo, &["rebase", "main"]);
    let before = fixture.head();

    fixture.assert_refused(DUPLICATE_MESSAGE, &before);
    fixture.assert_clean_without_rebase();
}

#[test]
fn rebase_replays_commits_that_fork_point_mode_would_drop() {
    let fixture = RebaseFixture::new("origin", "main");
    let repo = &fixture.repository.repo;
    git(repo, &["config", "rebase.forkPoint", "true"]);
    // Pushing records the commit in the remote-tracking ref's reflog, where fork-point mode
    // would find it after the remote branch is force-pushed without it and drop it. Git 2.54
    // keeps fork-point mode off for an explicit upstream even with this config; the rebase's
    // `--no-fork-point` keeps that true whatever the Git version.
    commit_file(
        repo,
        "dropped.txt",
        "dropped\n",
        "change dropped from the remote",
    );
    git(repo, &["push"]);
    commit_file(repo, "local.txt", "local\n", "local change");
    commit_file(&fixture.peer, "peer.txt", "peer\n", "peer change");
    git(&fixture.peer, &["push", "--force"]);
    let remote_tip = git_output(&fixture.peer, &["rev-parse", "HEAD"]);

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    assert_eq!(
        git_output(
            repo,
            &["rev-list", "--count", &format!("{remote_tip}..HEAD")]
        ),
        "2"
    );
    assert!(repo.join("dropped.txt").exists());
    assert!(repo.join("local.txt").exists());
}

#[test]
fn rebase_refuses_when_push_would_fall_back_to_a_missing_origin() {
    let fixture = RebaseFixture::new("upstream", "main");
    git(&fixture.repository.repo, &["branch", "--unset-upstream"]);
    fixture.publish("peer.txt", "peer\n", "peer change");
    let before = fixture.head();

    fixture.assert_refused(DIFFERENT_DESTINATION_MESSAGE, &before);
}

#[test]
fn rebase_uses_the_first_remote_when_origin_is_absent() {
    let fixture = RebaseFixture::new("upstream", "main");
    // The fixture's `push -u` makes the branch track `upstream`, which is where Push sends it.
    assert_eq!(
        git_output(
            &fixture.repository.repo,
            &["rev-parse", "--abbrev-ref", "@{upstream}"]
        ),
        "upstream/main"
    );
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");
    commit_file(
        &fixture.repository.repo,
        "local.txt",
        "local\n",
        "local change",
    );

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    assert_eq!(
        git_output(&fixture.repository.repo, &["rev-parse", "HEAD^"]),
        remote_tip
    );
}

#[test]
fn rebase_handles_a_branch_name_with_a_slash() {
    let fixture = RebaseFixture::new("origin", "feature/x");
    let remote_tip = fixture.publish("peer.txt", "peer\n", "peer change");
    commit_file(
        &fixture.repository.repo,
        "local.txt",
        "local\n",
        "local change",
    );

    fixture.runtime.rebase(fixture.cwd()).expect("rebase");

    assert_eq!(
        git_output(&fixture.repository.repo, &["rev-parse", "HEAD^"]),
        remote_tip
    );
    assert_eq!(
        git_output(
            &fixture.repository.repo,
            &["rev-parse", "refs/remotes/origin/feature/x"]
        ),
        remote_tip
    );
}

/// A checkout on `branch` that tracks the same-named branch of a bare `remote`, plus a peer
/// clone that publishes commits to it.
struct RebaseFixture {
    repository: Fixture,
    peer: PathBuf,
    runtime: LocalCheckout,
}

impl RebaseFixture {
    fn new(remote: &str, branch: &str) -> Self {
        let repository = Fixture::new();
        let temp = repository.temp.path();
        let bare = temp.join("remote.git");
        let bare_path = bare.to_str().expect("UTF-8 remote path");
        git(temp, &["init", "--bare", "-b", "main", bare_path]);
        git(&repository.repo, &["remote", "add", remote, bare_path]);
        git(&repository.repo, &["push", "-u", remote, "main"]);
        if branch != "main" {
            git(&repository.repo, &["checkout", "-b", branch]);
            git(&repository.repo, &["push", "-u", remote, branch]);
        }
        let peer = temp.join("peer");
        git(
            temp,
            &[
                "clone",
                "--branch",
                branch,
                bare_path,
                peer.to_str().expect("UTF-8 peer path"),
            ],
        );
        git(&peer, &["config", "user.email", "peer@example.test"]);
        git(&peer, &["config", "user.name", "Peer"]);
        let runtime = LocalCheckout::new(temp.join("managed"));
        Self {
            repository,
            peer,
            runtime,
        }
    }

    fn cwd(&self) -> &str {
        self.repository
            .repo
            .to_str()
            .expect("UTF-8 repository path")
    }

    fn head(&self) -> String {
        git_output(&self.repository.repo, &["rev-parse", "HEAD"])
    }

    /// Commit a file in the peer clone and push it; returns the new remote tip.
    fn publish(&self, file: &str, content: &str, message: &str) -> String {
        let commit = commit_file(&self.peer, file, content, message);
        git(&self.peer, &["push"]);
        commit
    }

    fn is_ancestor_of_head(&self, commit: &str) -> bool {
        Command::new("git")
            .args(["merge-base", "--is-ancestor", commit, "HEAD"])
            .current_dir(&self.repository.repo)
            .status()
            .expect("run git merge-base")
            .success()
    }

    fn git_path_exists(&self, name: &str) -> bool {
        let path = git_output(&self.repository.repo, &["rev-parse", "--git-path", name]);
        self.repository.repo.join(path).exists()
    }

    fn assert_refused(&self, message: &str, head: &str) {
        let error = self
            .runtime
            .rebase(self.cwd())
            .expect_err("rebase must be refused");
        assert_eq!(error.kind, CheckoutFailureKind::NotAllowed);
        assert_eq!(error.message, message);
        assert_eq!(self.head(), head);
    }

    fn assert_clean_without_rebase(&self) {
        assert!(git_output(&self.repository.repo, &["status", "--porcelain"]).is_empty());
        assert!(!self.git_path_exists("rebase-merge"));
        assert!(!self.git_path_exists("rebase-apply"));
    }
}

fn commit_file(cwd: &Path, file: &str, content: &str, message: &str) -> String {
    std::fs::write(cwd.join(file), content).expect("write committed file");
    git(cwd, &["add", file]);
    git(cwd, &["commit", "-m", message]);
    git_output(cwd, &["rev-parse", "HEAD"])
}

/// Commit on `main` and push it to origin, then return to `feature`; returns the commit.
fn publish_base_commit(repo: &Path) -> String {
    git(repo, &["checkout", "main"]);
    let commit = commit_file(repo, "base.txt", "base\n", "base change");
    git(repo, &["push", "origin", "main"]);
    git(repo, &["checkout", "feature"]);
    commit
}
