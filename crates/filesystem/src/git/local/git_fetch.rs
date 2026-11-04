//! Non-interactive, bounded background fetches of origin.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::git::ports::checkout::{CheckoutRuntime, CheckoutRuntimeError, CheckoutStatus};
use crate::git::ports::git_fetch::{GitFetchError, GitFetchRuntime};
use crate::support::process::{self, Bounds, RunError};

// A quiet fetch reports only errors; anything larger is not a fetch diagnostic.
const OUTPUT_LIMIT: u64 = 64 * 1024;

/// Local background Git adapter sharing checkout projection behavior.
#[derive(Debug)]
pub struct LocalGitFetch {
    checkout: super::checkout::LocalCheckout,
}

impl LocalGitFetch {
    /// Configure managed worktree ownership for checkout status events.
    #[must_use]
    pub fn new(managed_worktrees_root: PathBuf) -> Self {
        Self {
            checkout: super::checkout::LocalCheckout::new(managed_worktrees_root),
        }
    }
}

impl GitFetchRuntime for LocalGitFetch {
    fn repository(&self, cwd: &str) -> Result<Option<PathBuf>, GitFetchError> {
        let cwd = Path::new(cwd);
        let placement = match crate::support::git_command::run(
            cwd,
            &[
                "rev-parse",
                "--is-inside-work-tree",
                "--path-format=absolute",
                "--git-common-dir",
            ],
        ) {
            Ok(placement) => placement,
            Err(crate::support::git_command::GitError::Rejected) => return Ok(None),
            Err(crate::support::git_command::GitError::Io) => return Err(GitFetchError::Failed),
        };
        let Some(("true", common)) = placement.split_once('\n') else {
            return Ok(None);
        };
        let remotes = crate::support::git_command::run(cwd, &["remote"])
            .map_err(|_| GitFetchError::Failed)?;
        if !remotes.lines().any(|remote| remote == "origin") {
            return Ok(None);
        }
        std::fs::canonicalize(common)
            .map(Some)
            .map_err(|_| GitFetchError::Failed)
    }

    fn fetch(&self, cwd: &str, cancellation: &CancellationToken) -> Result<(), GitFetchError> {
        let mut command = fetch_command(cwd);
        run_fetch(&mut command, cancellation, Duration::from_secs(120))
    }

    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError> {
        self.checkout.status(cwd)
    }
}

fn fetch_command(cwd: &str) -> Command {
    let mut command = process::git_command(Path::new(cwd));
    command
        .args(["fetch", "origin", "--prune", "--quiet"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    command
}

fn run_fetch(
    command: &mut Command,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<(), GitFetchError> {
    let bounds = Bounds {
        timeout,
        stdout_limit: OUTPUT_LIMIT,
        stderr_limit: OUTPUT_LIMIT,
    };
    match process::run_bounded(command, bounds, Some(cancellation)) {
        Ok(output) if output.status.success() => Ok(()),
        Err(RunError::Cancelled) => Err(GitFetchError::Cancelled),
        Err(RunError::TimedOut) => Err(GitFetchError::TimedOut),
        Ok(_)
        | Err(
            RunError::Spawn(_)
            | RunError::Io(_)
            | RunError::StdoutExceeded
            | RunError::StderrExceeded,
        ) => Err(GitFetchError::Failed),
    }
}

#[cfg(test)]
mod tests;
