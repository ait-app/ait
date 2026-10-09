use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::{
    AntigravityClient,
    diagnostics::{Diagnostics, Failure},
};
use crate::ports::agent_session::AgentSessionError;

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
        kill_group(self.group);
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
        kill_group(self.group);
        self.stderr_reader.abort();
    }
}

fn command(client: &AntigravityClient, cwd: &Path) -> Command {
    let mut command = Command::new(&client.program);
    command
        .current_dir(cwd)
        .envs(client.environment.entries())
        .env("AGY_CLI_DISABLE_AUTO_UPDATE", "true")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub(super) async fn query(
    client: &AntigravityClient,
    cwd: &Path,
    args: &[&OsStr],
) -> Result<Vec<u8>, AgentSessionError> {
    let mut process = Process::spawn(command(client, cwd).args(args).stdin(Stdio::null()))?;
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
    failure_since: Option<Instant>,
}

impl Transport {
    pub(super) fn spawn(
        client: &AntigravityClient,
        cwd: &str,
        args: &[String],
    ) -> Result<Self, AgentSessionError> {
        let mut process = Process::spawn(
            command(client, Path::new(cwd))
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
            let mut output = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = (&mut output)
                    .take(MAX_FRAME as u64)
                    .read_until(b'\n', &mut bytes)
                    .await;
                if !matches!(read, Ok(1..)) || bytes.last() != Some(&b'\n') {
                    diagnostics.observe(if matches!(read, Ok(0)) {
                        Failure::Exit
                    } else {
                        Failure::Protocol
                    });
                    break;
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else {
                    diagnostics.observe(Failure::Protocol);
                    break;
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
            failure_since: None,
        })
    }

    pub(super) async fn receive(&mut self) -> Result<Value, AgentSessionError> {
        self.messages.recv().await.ok_or(AgentSessionError::Failed)
    }

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
        self.failure_since = None;
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

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        match self.messages.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected)
                if !self.closed && !self.process.stderr_reader.is_finished() =>
            {
                // stdout and stderr have independent readers. Give the final notice a bounded
                // chance to arrive before the session seals its failed tool entries.
                if self
                    .failure_since
                    .get_or_insert_with(Instant::now)
                    .elapsed()
                    < Duration::from_millis(100)
                {
                    Ok(None)
                } else {
                    Err(AgentSessionError::Failed)
                }
            }
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    pub(super) async fn interrupt(&self) -> Result<(), AgentSessionError> {
        #[cfg(unix)]
        {
            let id = self.process.child.id().ok_or(AgentSessionError::Failed)?;
            let status = Command::new("/bin/kill")
                .args(["-INT", &id.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map_err(|_| AgentSessionError::Failed)?;
            if status.success() {
                Ok(())
            } else {
                Err(AgentSessionError::Failed)
            }
        }
        #[cfg(not(unix))]
        {
            Err(AgentSessionError::Rejected)
        }
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

fn kill_group(group: Option<u32>) {
    #[cfg(unix)]
    if let Some(group) = group {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = group;
}
