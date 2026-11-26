use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{
    AntigravityClient,
    diagnostics::{Diagnostics, Failure},
};
use provider_core::ports::agent_session::AgentSessionError;
use provider_native::json_lines::{End, JsonLines};
use provider_native::process_group::{self, Signal};

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_EVENTS: usize = 128;

#[derive(Debug)]
struct Process {
    child: Child,
    group: Option<u32>,
    diagnostics: Diagnostics,
    stderr_reader: JoinHandle<()>,
}

impl Process {
    fn spawn(command: &mut Command) -> Result<Self, AgentSessionError> {
        command.kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            tracing::warn!(kind = ?error.kind(), "could not start AGY");
            AgentSessionError::Unavailable
        })?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let diagnostics = Diagnostics::default();
        let reader_diagnostics = diagnostics.clone();
        let stderr_reader = tokio::spawn(async move {
            reader_diagnostics.drain(stderr).await;
        });
        Ok(Self {
            group: child.id(),
            child,
            diagnostics,
            stderr_reader,
        })
    }

    async fn stop(&mut self) -> Result<(), AgentSessionError> {
        process_group::signal(self.group, Signal::Kill);
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        self.finish_diagnostics().await;
        Ok(())
    }

    async fn finish_diagnostics(&mut self) {
        if tokio::time::timeout(Duration::from_secs(2), &mut self.stderr_reader)
            .await
            .is_err()
        {
            self.stderr_reader.abort();
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        process_group::signal(self.group, Signal::Kill);
        self.stderr_reader.abort();
    }
}

fn command(
    client: &AntigravityClient,
    cwd: &Path,
    confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
) -> Result<Command, AgentSessionError> {
    let mut command = provider_native::sandbox::command(
        &client.program,
        confinement,
        provider_native::sandbox::ANTIGRAVITY_STATE,
        &|name| client.environment.variable(name),
    )?;
    command
        .current_dir(cwd)
        .envs(client.environment.entries())
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(command)
}

pub(super) async fn query(
    client: &AntigravityClient,
    cwd: &Path,
    args: &[&OsStr],
) -> Result<Vec<u8>, AgentSessionError> {
    let mut process = Process::spawn(command(client, cwd, None)?.args(args).stdin(Stdio::null()))?;
    let output = process
        .child
        .stdout
        .take()
        .ok_or(AgentSessionError::Failed)?;
    let result = tokio::time::timeout(client.deadline, async {
        let mut bytes = Vec::new();
        output
            .take((MAX_FRAME + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if bytes.len() > MAX_FRAME {
            return Err(AgentSessionError::Failed);
        }
        let status = process
            .child
            .wait()
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        if !status.success() {
            process.finish_diagnostics().await;
            tracing::warn!(%status, message = process.diagnostics.failure()
                .unwrap_or(Failure::Exit).message(), "AGY query failed");
            return Err(AgentSessionError::Failed);
        }
        Ok(bytes)
    })
    .await;
    if let Ok(result) = result {
        result
    } else {
        process.diagnostics.observe(Failure::Timeout);
        let _ = process.stop().await;
        tracing::warn!(message = Failure::Timeout.message(), "AGY query failed");
        Err(AgentSessionError::Failed)
    }
}

#[derive(Debug)]
pub(super) struct Transport {
    process: Process,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    deadline: Duration,
    closed: bool,
}

impl Transport {
    pub(super) fn spawn(
        client: &AntigravityClient,
        cwd: &str,
        args: &[String],
        confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
    ) -> Result<Self, AgentSessionError> {
        let mut process = Process::spawn(
            command(client, Path::new(cwd), confinement)?
                .args(args)
                .stdin(Stdio::piped()),
        )?;
        let input = process
            .child
            .stdin
            .take()
            .ok_or(AgentSessionError::Failed)?;
        let output = process
            .child
            .stdout
            .take()
            .ok_or(AgentSessionError::Failed)?;
        let (sender, messages) = mpsc::channel(MAX_EVENTS);
        let diagnostics = process.diagnostics.clone();
        let reader = tokio::spawn(async move {
            let mut output = JsonLines::new(output, MAX_FRAME);
            loop {
                let message = match output.next().await {
                    Ok(frame) => frame.value,
                    Err(End::Closed) => break diagnostics.observe(Failure::Exit),
                    Err(End::Read | End::Decode) => break diagnostics.observe(Failure::Protocol),
                };
                if !message["event"].is_string() {
                    diagnostics.observe(Failure::Protocol);
                    break;
                }
                if sender.send(message).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            process,
            input: Some(input),
            messages,
            reader,
            deadline: client.deadline,
            closed: false,
        })
    }

    pub(super) async fn receive(&mut self) -> Result<Value, AgentSessionError> {
        self.messages.recv().await.ok_or(AgentSessionError::Failed)
    }

    #[cfg(test)]
    pub(super) fn failure(&self) -> Option<Failure> {
        self.process.diagnostics.failure()
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Rejected)?;
        if bytes.len() >= MAX_FRAME {
            return Err(AgentSessionError::Rejected);
        }
        bytes.push(b'\n');
        self.process.diagnostics.clear();
        let input = self.input.as_mut().ok_or(AgentSessionError::Failed)?;
        let result = tokio::time::timeout(self.deadline, async {
            input.write_all(&bytes).await?;
            input.flush().await
        })
        .await;
        if matches!(result, Ok(Ok(()))) {
            return Ok(());
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.input.take();
        self.reader.abort();
        if matches!(
            tokio::time::timeout(Duration::from_secs(2), self.process.child.wait()).await,
            Ok(Ok(_))
        ) {
            self.process.finish_diagnostics().await;
            return Ok(());
        }
        self.process.stop().await
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

#[cfg(all(test, unix))]
mod tests;
