//! Pull-request Git placement, tracking and push refs without changing existing branches.

use super::{CreatePlan, Path, READ_TIMEOUT, WRITE_TIMEOUT, WorktreeError};
use super::{default_branch, git, optional_git, unique_branch, validate_branch};
use crate::worktrees::ports::worktrees::{ChangeRequestCheckout, ChangeRequestCheckoutRef};

pub(super) fn plan(
    repo: &Path,
    target: &ChangeRequestCheckout,
) -> Result<CreatePlan, WorktreeError> {
    validate_branch(repo, &target.head_ref)?;
    validate_branch(repo, &target.local_branch)?;
    let base = if target.base_ref.is_empty() {
        default_branch(repo)?
    } else {
        target.base_ref.clone()
    };
    if base == "HEAD" {
        return Err(WorktreeError::Invalid(
            "Pull request base cannot be HEAD".into(),
        ));
    }
    validate_branch(repo, &base)?;
    let branch = unique_branch(repo, &target.local_branch);
    fetch_checkout(repo, &target.checkout_refs, &branch)?;
    Ok(CreatePlan {
        branch_name: branch.clone(),
        comparison_base_ref: Some(base),
        arguments: vec![branch],
    })
}

fn fetch_checkout(
    repo: &Path,
    refs: &[ChangeRequestCheckoutRef],
    branch: &str,
) -> Result<(), WorktreeError> {
    for candidate in refs {
        validate_branch(repo, &candidate.remote)?;
        git(
            repo,
            &["check-ref-format", &candidate.reference],
            READ_TIMEOUT,
            &[0],
        )?;
    }
    let temporary = format!("refs/ait/checkout/{}", uuid::Uuid::new_v4());
    let result = fetch_branch(repo, branch, &temporary, refs);
    let cleanup = git(repo, &["update-ref", "-d", &temporary], READ_TIMEOUT, &[0]);
    result.and(cleanup.map(|_| ()))
}

fn fetch_branch(
    repo: &Path,
    branch: &str,
    temporary: &str,
    refs: &[ChangeRequestCheckoutRef],
) -> Result<(), WorktreeError> {
    for candidate in refs {
        let specification = format!("{}:{temporary}", candidate.reference);
        let output = git(
            repo,
            &["fetch", "--no-tags", &candidate.remote, &specification],
            WRITE_TIMEOUT,
            &[0, 1, 128],
        )?;
        if output.status.success() {
            // Do not force-update an existing branch if another creation won the race.
            return git(
                repo,
                &["branch", "--no-track", branch, temporary],
                READ_TIMEOUT,
                &[0],
            )
            .map(|_| ());
        }
    }
    Err(WorktreeError::Io(
        "Unable to fetch change request from its forge-provided refs".into(),
    ))
}

pub(super) fn configure(
    cwd: &Path,
    branch: &str,
    target: &ChangeRequestCheckout,
) -> Result<(), WorktreeError> {
    let remote = format!("paseo-pr-{}", target.number);
    if let Some(url) = &target.push_remote_url {
        push_remote(cwd, branch, &remote, url, &target.head_ref)?;
        let fetch = format!(
            "+refs/heads/{}:refs/remotes/{remote}/{}",
            target.head_ref, target.head_ref
        );
        set(cwd, &format!("remote.{remote}.fetch"), &fetch)?;
        track(cwd, branch, &remote, &target.head_ref)?;
    } else if target.track_origin {
        track(cwd, branch, "origin", &target.head_ref)?;
        if branch != target.head_ref
            && let Some(url) = optional_git(cwd, &["remote", "get-url", "--push", "origin"])
        {
            push_remote(cwd, branch, &remote, &url, &target.head_ref)?;
        }
    }
    Ok(())
}

fn push_remote(
    cwd: &Path,
    branch: &str,
    remote: &str,
    url: &str,
    head: &str,
) -> Result<(), WorktreeError> {
    set(cwd, &format!("remote.{remote}.url"), url)?;
    set(
        cwd,
        &format!("remote.{remote}.push"),
        &format!("HEAD:refs/heads/{head}"),
    )?;
    set(cwd, &format!("branch.{branch}.pushRemote"), remote)
}

fn track(cwd: &Path, branch: &str, remote: &str, head: &str) -> Result<(), WorktreeError> {
    let fetch = format!("+refs/heads/{head}:refs/remotes/{remote}/{head}");
    let output = git(
        cwd,
        &["fetch", "--no-tags", remote, &fetch],
        WRITE_TIMEOUT,
        &[0, 1, 128],
    )?;
    if output.status.success() {
        git(
            cwd,
            &[
                "branch",
                "--set-upstream-to",
                &format!("{remote}/{head}"),
                branch,
            ],
            READ_TIMEOUT,
            &[0],
        )?;
    }
    Ok(())
}

fn set(cwd: &Path, key: &str, value: &str) -> Result<(), WorktreeError> {
    git(cwd, &["config", key, value], READ_TIMEOUT, &[0]).map(|_| ())
}
