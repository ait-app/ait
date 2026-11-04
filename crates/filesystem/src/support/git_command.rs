//! Bounded Git runner for short, fixed read and mutation invocations.

use std::path::Path;
use std::time::Duration;

use crate::support::process::{self, Bounds};

const TIMEOUT: Duration = Duration::from_secs(3);
const OUTPUT_LIMIT: u64 = 16 * 1024;
const STDERR_LIMIT: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitError {
    Io,
    Rejected,
}

// Fixed Git invocations with bounded output and lifetime; callers select reads or mutations.
pub(crate) fn run(root: &Path, arguments: &[&str]) -> Result<String, GitError> {
    run_with_bounds(root, arguments, OUTPUT_LIMIT, TIMEOUT)
}

// Like `run`, for listings whose size grows with the repository.
pub(crate) fn run_with_bounds(
    root: &Path,
    arguments: &[&str],
    output_limit: u64,
    timeout: Duration,
) -> Result<String, GitError> {
    let mut command = process::git_command(root);
    command
        .arg("--no-optional-locks")
        .args(["-c", "core.fsmonitor=false"])
        .args(arguments);
    let output = process::run_bounded(
        &mut command,
        Bounds {
            timeout,
            stdout_limit: output_limit,
            stderr_limit: STDERR_LIMIT,
        },
        None,
    )
    .map_err(|_| GitError::Io)?;
    if !output.status.success() {
        return Err(GitError::Rejected);
    }
    let mut text = String::from_utf8(output.stdout).map_err(|_| GitError::Io)?;
    if text.ends_with('\n') {
        text.pop();
    }
    Ok(text)
}
