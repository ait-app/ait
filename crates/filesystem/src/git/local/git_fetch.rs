//! Non-interactive, bounded background fetches of origin.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::git::ports::checkout::{CheckoutRuntime, CheckoutRuntimeError, CheckoutStatus};
use crate::git::ports::git_fetch::{GitFetchError, GitFetchRuntime};

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
    let mut command = Command::new("git");
    command
        .args(["fetch", "origin", "--prune", "--quiet"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

fn run_fetch(
    command: &mut Command,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<(), GitFetchError> {
    if cancellation.is_cancelled() {
        return Err(GitFetchError::Cancelled);
    }
    let mut child = command.spawn().map_err(|_| GitFetchError::Failed)?;
    let deadline = Instant::now() + timeout;
    loop {
        let failure = if cancellation.is_cancelled() {
            Some(GitFetchError::Cancelled)
        } else if Instant::now() >= deadline {
            Some(GitFetchError::TimedOut)
        } else {
            None
        };
        if let Some(failure) = failure {
            terminate(&mut child);
            return Err(failure);
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(GitFetchError::Failed)
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                terminate(&mut child);
                return Err(GitFetchError::Failed);
            }
        }
    }
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    let _ = Command::new("/bin/kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests;
