//! Bounded one-shot native commands with cancellation-safe descendant ownership.
use std::process::Stdio;

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

pub(super) async fn output(command: &mut Command) -> Result<String, AgentSessionError> {
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
        .take(MAX_OUTPUT + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() as u64 > MAX_OUTPUT
        || !process
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

#[cfg(test)]
mod tests;
