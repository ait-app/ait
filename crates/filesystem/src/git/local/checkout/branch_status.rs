//! Same-named remote branch and index facts for workspace actions.

use std::path::Path;

use crate::git::ports::checkout::{CheckoutBranchStatus, CheckoutRuntimeError};

use super::{compare_refs, git_optional};

pub(super) fn read(
    cwd: &Path,
    branch: Option<&str>,
    remote: Option<&str>,
    has_conflicts: bool,
) -> Result<CheckoutBranchStatus, CheckoutRuntimeError> {
    let head_sha = git_optional(cwd, &["rev-parse", "--verify", "HEAD"])?;
    let remote_ref = match (remote, branch) {
        (Some(remote), Some(branch)) => {
            let reference = format!("refs/remotes/{remote}/{branch}");
            git_optional(cwd, &["rev-parse", "--verify", &reference])?.map(|_| reference)
        }
        _ => None,
    };
    let ahead_behind = remote_ref
        .as_deref()
        .map(|reference| compare_refs(cwd, reference, "HEAD"))
        .transpose()?
        .flatten();
    Ok(CheckoutBranchStatus {
        head_sha,
        has_conflicts,
        remote_ref,
        ahead_behind,
    })
}

/// Whether `git status --porcelain=v1` output lists an unmerged path.
pub(super) fn porcelain_has_conflicts(working_status: &str) -> bool {
    working_status.lines().any(|line| {
        matches!(
            line.get(..2),
            Some("DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
        )
    })
}

#[cfg(test)]
mod tests;
