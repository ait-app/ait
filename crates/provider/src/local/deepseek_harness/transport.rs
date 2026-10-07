use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::DeepSeekHarnessClient;
use crate::ports::agent_session::AgentSessionError;

const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_EVENTS: usize = 128;

#[derive(Debug)]
pub(super) struct Transport {
    child: Child,
    input: ChildStdin,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    events: VecDeque<Value>,
    sequence: u64,
    deadline: Duration,
    closed: bool,
}

impl Transport {
    pub(super) fn spawn(
        client: &DeepSeekHarnessClient,
        cwd: &str,
    ) -> Result<Self, AgentSessionError> {
        let mut command = Command::new(&client.program);
        command
            .args(["--profile", "acp"])
            .current_dir(cwd)
            .envs(client.environment.entries())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command.spawn().map_err(|error| {
            tracing::warn!(
                error_kind = ?error.kind(),
                os_error = error.raw_os_error(),
                "failed to spawn DeepSeek Harness ACP process"
            );
            AgentSessionError::Unavailable
        })?;
        let input = child.stdin.take().ok_or(AgentSessionError::Failed)?;
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let (sender, messages) = mpsc::channel(MAX_EVENTS);
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                let read = (&mut output)
                    .take(MAX_FRAME as u64)
                    .read_until(b'\n', &mut bytes)
                    .await;
                if !matches!(read, Ok(1..)) || bytes.last() != Some(&b'\n') {
                    break;
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else {
                    break;
                };
                if message["jsonrpc"] != "2.0" || sender.send(message).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            messages,
            reader,
            events: VecDeque::new(),
            sequence: 0,
            deadline: client.deadline,
            closed: false,
        })
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Failed)?;
        if bytes.len() >= 64 * 1024 * 1024 {
            return Err(AgentSessionError::Rejected);
        }
        bytes.push(b'\n');
        let result = tokio::time::timeout(self.deadline, async {
            self.input.write_all(&bytes).await?;
            self.input.flush().await
        })
        .await;
        if matches!(result, Ok(Ok(()))) {
            return Ok(());
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    pub(super) async fn begin(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<String, AgentSessionError> {
        self.sequence += 1;
        let id = format!("ait-{}", self.sequence);
        self.send(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        Ok(id)
    }

    pub(super) async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, AgentSessionError> {
        let id = self.begin(method, params).await?;
        let result = tokio::time::timeout(self.deadline, async {
            loop {
                let message = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                if message.get("method").is_none() && message["id"] == id {
                    return response(&message);
                }
                // No prompt runs during configuration/discovery. Reject unsupported callbacks
                // so a provider cannot deadlock initialization waiting for an answer.
                if message.get("id").is_some() && message["method"].is_string() {
                    self.send(&json!({"jsonrpc":"2.0","id":message["id"],
                        "error":{"code":-32601,"message":"Unsupported client method"}}))
                        .await?;
                    continue;
                }
                if self.events.len() >= MAX_EVENTS {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(message);
            }
        })
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(AgentSessionError::Rejected)) => Err(AgentSessionError::Rejected),
            _ => {
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
        }
    }

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if let Some(message) = self.events.pop_front() {
            return Ok(Some(message));
        }
        match self.messages.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.reader.abort();
        kill_group(&self.child);
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        kill_group(&self.child);
    }
}

fn kill_group(child: &Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{id}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = child;
}

pub(super) fn response(message: &Value) -> Result<Value, AgentSessionError> {
    if message.get("result").is_some() && message.get("error").is_none() {
        return Ok(message["result"].clone());
    }
    if message["error"]["code"].as_i64().is_some() && message.get("result").is_none() {
        return Err(AgentSessionError::Rejected);
    }
    Err(AgentSessionError::Failed)
}
