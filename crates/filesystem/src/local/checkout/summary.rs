//! Lightweight sidebar statistics without allocating structured diff hunks.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use model::workspace::runtime::{WorkspaceDiffStat, WorkspaceGitSnapshot};

use super::{
    CheckoutFailureKind, CheckoutRuntime, CheckoutRuntimeError, DIFF_OUTPUT_LIMIT, LocalCheckout,
    SMALL_OUTPUT_LIMIT, checkout_error, comparison_base, diff_head, git_optional, git_required,
    require_git_directory, validate_relative_path,
};

#[derive(Debug, Default)]
/// Checkout presentation facts and commit identity used to invalidate Forge observations.
pub(crate) struct GitSummary {
    /// Latest Git facts, absent for a non-Git directory.
    pub(crate) git: Option<WorkspaceGitSnapshot>,
    /// Current commit, absent for an unborn checkout.
    pub(crate) head: Option<String>,
}

impl LocalCheckout {
    /// Read Git status and bounded sidebar statistics for `cwd` without constructing diff hunks.
    /// Non-Git directories return an empty summary; unavailable statistics preserve Git status.
    ///
    /// # Errors
    /// Returns invalid-directory, Git-command, or bounded-output failures from status/identity reads.
    pub(crate) fn sidebar_summary(&self, cwd: &str) -> Result<GitSummary, CheckoutRuntimeError> {
        let status = self.status(cwd)?;
        if !status.is_git {
            return Ok(GitSummary::default());
        }
        let cwd = require_git_directory(cwd)?;
        let head = git_optional(&cwd, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        let comparison = match (&status.base_ref, &status.current_branch) {
            (Some(base), Some(branch)) if base != branch => {
                let base = comparison_base(&cwd, base)?;
                git_optional(&cwd, &["merge-base", &base, "HEAD"])?
            }
            (_, Some(branch)) => git_optional(
                &cwd,
                &[
                    "merge-base",
                    &format!("refs/remotes/origin/{branch}"),
                    "HEAD",
                ],
            )?,
            (_, None) => None,
        };
        // Local-only and unborn repositories still expose their working changes.
        let comparison = comparison.map_or_else(|| diff_head(&cwd), Ok)?;
        let diff_stat = statistics(&cwd, &comparison).ok();
        Ok(GitSummary {
            git: Some(WorkspaceGitSnapshot {
                current_branch: status.current_branch,
                remote_url: status.remote_url,
                is_managed_worktree: status.is_managed_worktree,
                is_dirty: status.is_dirty,
                ahead_behind: status
                    .ahead_behind
                    .map(|counts| (counts.ahead, counts.behind)),
                ahead_of_origin: status.ahead_of_origin,
                behind_of_origin: status.behind_of_origin,
                diff_stat: diff_stat.filter(|stat| stat.additions != 0 || stat.deletions != 0),
            }),
            head,
        })
    }
}

fn statistics(cwd: &Path, comparison: &str) -> Result<WorkspaceDiffStat, CheckoutRuntimeError> {
    let tracked = git_required(
        cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--numstat",
            "-z",
            comparison,
            "--",
        ],
        DIFF_OUTPUT_LIMIT,
    )?;
    let mut result = parse_numstat(&tracked);
    let untracked = git_required(
        cwd,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        SMALL_OUTPUT_LIMIT,
    )?;
    let mut remaining = DIFF_OUTPUT_LIMIT;
    for path in untracked.split('\0').filter(|path| !path.is_empty()) {
        validate_relative_path(path)?;
        result.additions = result
            .additions
            .saturating_add(count_lines(&cwd.join(path), &mut remaining)?);
    }
    Ok(result)
}

fn parse_numstat(output: &str) -> WorkspaceDiffStat {
    let mut result = WorkspaceDiffStat::default();
    let mut records = output.split('\0');
    while let Some(record) = records.next() {
        let mut fields = record.splitn(3, '\t');
        let added = fields.next().and_then(|value| value.parse::<u64>().ok());
        let removed = fields.next().and_then(|value| value.parse::<u64>().ok());
        if let (Some(added), Some(removed)) = (added, removed) {
            result.additions = result.additions.saturating_add(added);
            result.deletions = result.deletions.saturating_add(removed);
        }
        if fields.next() == Some("") {
            records.next();
            records.next();
        }
    }
    result
}

fn count_lines(path: &Path, remaining: &mut u64) -> Result<u64, CheckoutRuntimeError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| super::io_error(&error))?;
    if metadata.file_type().is_symlink() {
        return Ok(1);
    }
    if !metadata.is_file() {
        return Ok(0);
    }
    let mut file = File::open(path).map_err(|error| super::io_error(&error))?;
    let mut buffer = [0; 8192];
    let mut lines = 0_u64;
    let mut last = None;
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|error| super::io_error(&error))?;
        if length == 0 {
            return Ok(lines + u64::from(last.is_some_and(|byte| byte != b'\n')));
        }
        *remaining = remaining.checked_sub(length as u64).ok_or_else(|| {
            checkout_error(
                CheckoutFailureKind::Unknown,
                "Untracked statistics exceeded the read budget",
            )
        })?;
        let bytes = &buffer[..length];
        if bytes.contains(&0) {
            return Ok(0);
        }
        lines += memchr::memchr_iter(b'\n', bytes).count() as u64;
        last = bytes.last().copied();
    }
}

#[cfg(test)]
mod tests;
