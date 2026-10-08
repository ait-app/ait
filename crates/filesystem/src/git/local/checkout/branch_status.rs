//! Same-named remote branch and index facts for workspace actions.

use std::path::Path;

use crate::git::ports::checkout::{CheckoutBranchStatus, CheckoutRuntimeError};

use super::{compare_refs, git_optional};

pub(super) fn read(
    cwd: &Path,
    branch: Option<&str>,
    remote: Option<&str>,
    working_status: &str,
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
        has_conflicts: working_status.lines().any(|line| {
            matches!(
                line.get(..2),
                Some("DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
            )
        }),
        remote_ref,
        ahead_behind,
    })
}

#[cfg(test)]
mod tests;
