//! Bounded one-shot native commands with cancellation-safe descendant ownership.
use std::process::{ExitStatus, Stdio};

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::ports::agent_session::AgentSessionError;

const MAX_OUTPUT: u64 = 1024 * 1024;

struct Process {
    child: Child,
    #[cfg(unix)]
    group: Option<u32>,
}

impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.group {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{group}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.start_kill();
    }
}

/// Run `command` once and return its UTF-8 stdout, bounded to 1 MiB.
///
/// # Errors
///
/// Returns `AgentSessionError::Unavailable` when the command cannot start, and
/// `AgentSessionError::Failed` for read failures, oversized or non-UTF-8 output, or an
/// unsuccessful exit status.
pub(super) async fn output(command: &mut Command) -> Result<String, AgentSessionError> {
    let (status, bytes) = bounded_output(command, MAX_OUTPUT).await?;
    if !status.success() {
        return Err(AgentSessionError::Failed);
    }
    String::from_utf8(bytes).map_err(|_| AgentSessionError::Failed)
}

/// Run `command` once and return its exit status with at most `limit` bytes of stdout.
///
/// stdin and stderr are null. The command runs in its own process group, which is killed when
/// the returned future is dropped, so callers may bound it with a deadline.
///
/// # Arguments
///
/// * `command` - Configured native command; its stdio and process group are overridden.
/// * `limit` - Largest accepted stdout size in bytes.
///
/// # Returns
///
/// The exit status and the complete stdout, whatever the status.
///
/// # Errors
///
/// Returns `AgentSessionError::Unavailable` when the command cannot start, and
/// `AgentSessionError::Failed` for read or wait failures or stdout exceeding `limit`.
pub(super) async fn bounded_output(
    command: &mut Command,
    limit: u64,
) -> Result<(ExitStatus, Vec<u8>), AgentSessionError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let child = command
        .spawn()
        .map_err(|_| AgentSessionError::Unavailable)?;
    let mut process = Process {
        #[cfg(unix)]
        group: child.id(),
        child,
    };
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or(AgentSessionError::Failed)?;
    let mut bytes = Vec::new();
    stdout
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > limit {
        return Err(AgentSessionError::Failed);
    }
    let status = process
        .child
        .wait()
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    Ok((status, bytes))
}

#[cfg(test)]
mod tests;
