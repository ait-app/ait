//! Rebase of the current branch onto its same-named branch on the preferred remote.

use std::path::Path;

use crate::git::ports::checkout::{CheckoutFailureKind, CheckoutRuntimeError};

use super::{
    GitRunOptions, SMALL_OUTPUT_LIMIT, WRITE_TIMEOUT, checkout_error, configured_push_target,
    current_branch, git_required, git_write, git_write_noninteractive, has_unmerged_paths, lines,
    preferred_remote, remote_upstream, require_git_directory, run_git, run_git_with_interaction,
};

/// Prints the paths of the Git state files that exist while a rebase, merge, cherry-pick, revert
/// or multi-commit sequence is in progress. The rebase state comes first.
const OPERATION_STATE_PATHS_ARGUMENTS: [&str; 13] = [
    "rev-parse",
    "--git-path",
    "rebase-merge",
    "--git-path",
    "rebase-apply",
    "--git-path",
    "MERGE_HEAD",
    "--git-path",
    "CHERRY_PICK_HEAD",
    "--git-path",
    "REVERT_HEAD",
    "--git-path",
    "sequencer",
];

/// Prints the paths of the Git state files that exist while a rebase is in progress.
const REBASE_STATE_PATHS_ARGUMENTS: &[&str] = OPERATION_STATE_PATHS_ARGUMENTS.split_at(5).0;

const DUPLICATE_MESSAGE: &str = "Rebasing would duplicate commits that are already published, so this branch cannot be rebased here";

/// Fetch the current branch's same-named remote branch and rebase onto it, keeping merges.
///
/// Every refusal but one happens before the checkout changes: a successful rebase that replaced a
/// published commit is reset to the original HEAD and refused. Only a rebase this call started is
/// aborted.
///
/// # Errors
/// Returns `NotAllowed` refusals, `MergeConflict` when the rebase stopped and was undone, and
/// passes other fetch, Git, timeout, or output failures through.
pub(super) fn rebase(cwd: &str) -> Result<(), CheckoutRuntimeError> {
    let cwd = require_git_directory(cwd)?;
    require_no_operation_in_progress(&cwd)?;
    let branch = current_branch(&cwd, "rebase")?;
    let remotes = lines(&git_required(&cwd, &["remote"], SMALL_OUTPUT_LIMIT)?);
    let remote = preferred_remote(&remotes).ok_or_else(|| refused("No remote is configured"))?;
    require_no_tracked_changes(&cwd)?;
    require_push_destination(&cwd, remote, &branch)?;
    let target = format!("refs/remotes/{remote}/{branch}");
    fetch_branch(&cwd, remote, &branch, &target)?;
    rebase_onto(&cwd, &target)
}

fn require_no_operation_in_progress(cwd: &Path) -> Result<(), CheckoutRuntimeError> {
    if has_unmerged_paths(cwd) || any_state_exists(cwd, &OPERATION_STATE_PATHS_ARGUMENTS)? {
        return Err(refused(
            "Finish or abort the operation in progress before rebasing",
        ));
    }
    Ok(())
}

/// Whether any Git state file printed by `rev-parse --git-path` `arguments` exists.
fn any_state_exists(cwd: &Path, arguments: &[&str]) -> Result<bool, CheckoutRuntimeError> {
    let paths = git_required(cwd, arguments, SMALL_OUTPUT_LIMIT)?;
    Ok(paths.lines().any(|path| cwd.join(path).exists()))
}

fn require_no_tracked_changes(cwd: &Path) -> Result<(), CheckoutRuntimeError> {
    let diff = run_git(
        cwd,
        &[
            "diff",
            "--quiet",
            "--no-ext-diff",
            "--ignore-submodules=untracked",
            "HEAD",
            "--",
        ],
        &[0, 1],
        SMALL_OUTPUT_LIMIT,
    )?;
    if diff.exit_code == 0 {
        Ok(())
    } else {
        Err(refused("Commit or stash your changes before rebasing"))
    }
}

/// Refuse when `push` would publish this branch anywhere but `remote`'s same-named branch.
///
/// The destination is resolved as `push` resolves it, down to its final fallback: without a
/// configured push target or upstream, `push` publishes the same-named branch on `origin`.
fn require_push_destination(
    cwd: &Path,
    remote: &str,
    branch: &str,
) -> Result<(), CheckoutRuntimeError> {
    let destination = match configured_push_target(cwd, branch)? {
        Some(destination) => Some(destination),
        None => remote_upstream(cwd, branch)?,
    };
    let matches = destination.map_or(remote == "origin", |(push_remote, head)| {
        push_remote == remote && head == branch
    });
    if !matches {
        return Err(refused(
            "This branch is pushed to a different remote branch, so it cannot be rebased here",
        ));
    }
    Ok(())
}

fn fetch_branch(
    cwd: &Path,
    remote: &str,
    branch: &str,
    target: &str,
) -> Result<(), CheckoutRuntimeError> {
    let refspec = format!("+refs/heads/{branch}:{target}");
    let Err(error) = git_write_noninteractive(cwd, &["fetch", "--no-tags", remote, &refspec])
    else {
        return Ok(());
    };
    if remote_lacks_branch(cwd, remote, branch) {
        return Err(refused(format!("The remote has no branch named {branch}")));
    }
    Err(error)
}

/// Whether `remote` answers and has no `branch`; checked only after a fetch failed, so the
/// answer does not depend on the wording of Git's localized fetch error.
fn remote_lacks_branch(cwd: &Path, remote: &str, branch: &str) -> bool {
    run_git_with_interaction(
        cwd,
        &[
            "ls-remote",
            "--exit-code",
            remote,
            &format!("refs/heads/{branch}"),
        ],
        &[0, 2],
        SMALL_OUTPUT_LIMIT,
        GitRunOptions {
            timeout: WRITE_TIMEOUT,
            noninteractive: true,
        },
    )
    .is_ok_and(|output| output.exit_code == 2)
}

fn rebase_onto(cwd: &Path, target: &str) -> Result<(), CheckoutRuntimeError> {
    let original_head = git_required(cwd, &["rev-parse", "--verify", "HEAD"], SMALL_OUTPUT_LIMIT)?;
    // Other local branches must not move: an undone rebase restores only this branch. Fork-point
    // mode could drop commits found only in the remote-tracking ref's reflog.
    let rebase = git_write(
        cwd,
        &[
            "-c",
            "rebase.updateRefs=false",
            "rebase",
            "--rebase-merges",
            "--no-fork-point",
            target,
        ],
    );
    let Err(error) = rebase else {
        return keep_published_commits(cwd, &original_head);
    };
    let conflicted = has_unmerged_paths(cwd);
    abort_stopped_rebase(cwd, conflicted)?;
    if conflicted {
        Err(checkout_error(
            CheckoutFailureKind::MergeConflict,
            "Rebase hit conflicts and was undone",
        ))
    } else {
        Err(error)
    }
}

/// Abort the rebase left behind by a failed `git rebase`, if it started one.
///
/// No rebase was in progress before, so a remaining one is the rebase this call started.
fn abort_stopped_rebase(cwd: &Path, conflicted: bool) -> Result<(), CheckoutRuntimeError> {
    if !any_state_exists(cwd, REBASE_STATE_PATHS_ARGUMENTS)? {
        return Ok(());
    }
    git_write(cwd, &["rebase", "--abort"])
        .map(|_| ())
        .map_err(|abort| {
            let kind = if conflicted {
                CheckoutFailureKind::MergeConflict
            } else {
                abort.kind
            };
            checkout_error(
                kind,
                format!("Rebase stopped and could not be undone: {}", abort.message),
            )
        })
}

/// Reset to `original_head` and refuse when the rebase replaced a published commit.
///
/// An unverified rebase is undone rather than left in place.
fn keep_published_commits(cwd: &Path, original_head: &str) -> Result<(), CheckoutRuntimeError> {
    let error = match replaced_published_commit(cwd, original_head) {
        Ok(false) => return Ok(()),
        Ok(true) => refused(DUPLICATE_MESSAGE),
        Err(error) => error,
    };
    match git_write(cwd, &["reset", "--hard", original_head]) {
        Ok(_) => Err(error),
        Err(reset) => Err(checkout_error(
            error.kind,
            format!(
                "{}; undoing the rebase failed: {}",
                error.message, reset.message
            ),
        )),
    }
}

/// Whether a commit that left the branch is contained in a remote-tracking ref: counting the
/// commits of `original_head` missing from HEAD with and without those refs excluded differs
/// exactly then.
fn replaced_published_commit(
    cwd: &Path,
    original_head: &str,
) -> Result<bool, CheckoutRuntimeError> {
    let arguments = [
        "rev-list",
        "--count",
        original_head,
        "--not",
        "HEAD",
        "--remotes",
    ];
    Ok(commit_count(cwd, &arguments[..5])? != commit_count(cwd, &arguments)?)
}

fn commit_count(cwd: &Path, arguments: &[&str]) -> Result<u64, CheckoutRuntimeError> {
    git_required(cwd, arguments, SMALL_OUTPUT_LIMIT)?
        .trim()
        .parse()
        .map_err(|_| checkout_error(CheckoutFailureKind::Unknown, "Invalid Git commit count"))
}

fn refused(message: impl Into<String>) -> CheckoutRuntimeError {
    checkout_error(CheckoutFailureKind::NotAllowed, message)
}
