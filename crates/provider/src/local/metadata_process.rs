//! Bounded one-shot native commands with cancellation-safe descendant ownership.
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use super::process_group::{self, Signal};
use crate::ports::agent_session::AgentSessionError;

const MAX_OUTPUT: u64 = 1024 * 1024;

/// One-shot native command whose process group is killed when it is dropped.
pub(super) struct Process {
    child: Child,
    group: Option<u32>,
}

impl Drop for Process {
    fn drop(&mut self) {
        process_group::signal(self.group, Signal::Kill);
        let _ = self.child.start_kill();
    }
}

impl Process {
    /// Start `command` in its own process group with null stdin/stderr and piped stdout.
    ///
    /// # Errors
    ///
    /// Returns the OS launch error so callers can classify an unusable executable.
    pub(super) fn spawn(command: &mut Command) -> std::io::Result<Self> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn()?;
        Ok(Self {
            group: child.id(),
            child,
        })
    }

    /// Read at most `limit` bytes of UTF-8 stdout and require a successful exit.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` for oversized, non-UTF-8 or unreadable output
    /// and for a non-zero exit status.
    pub(super) async fn output(mut self, limit: u64) -> Result<String, AgentSessionError> {
        let stdout = self.child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if bytes.len() as u64 > limit
            || !self
                .child
                .wait()
                .await
                .map_err(|_| AgentSessionError::Failed)?
                .success()
        {
            return Err(AgentSessionError::Failed);
        }
        String::from_utf8(bytes).map_err(|_| AgentSessionError::Failed)
    }
}

/// Run `command` and return at most one MiB of successful UTF-8 stdout.
///
/// # Errors
///
/// Returns `AgentSessionError::Unavailable` when the command cannot start, otherwise the
/// output failures of [`Process::output`].
pub(super) async fn output(command: &mut Command) -> Result<String, AgentSessionError> {
    Process::spawn(command)
        .map_err(|_| AgentSessionError::Unavailable)?
        .output(MAX_OUTPUT)
        .await
}

#[cfg(test)]
mod tests;
