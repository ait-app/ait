use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;

use super::OmpClient;
use crate::local::diagnostics::Diagnostics;
use crate::ports::agent_session::AgentSessionError;
use crate::ports::diagnostics::{Category, Diagnostic};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_BUFFER: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub(super) struct Transport {
    child: Child,
    pub(super) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    group: Option<u32>,
    input: Option<ChildStdin>,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    events: VecDeque<Value>,
    buffered_bytes: usize,
    sequence: u64,
    deadline: Duration,
    closed: bool,
}

impl Transport {
    pub(super) fn spawn(
        client: &OmpClient,
        cwd: &str,
        args: &[String],
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = Diagnostics::new("Oh My Pi");
        let mut command = Command::new(&client.program);
        command
            .args(args)
            .current_dir(cwd)
            .envs(client.environment.entries())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|error| diagnostics.spawn_error(&error))?;
        let input = child.stdin.take().ok_or(AgentSessionError::Failed)?;
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let stderr_diagnostics = diagnostics.clone();
        let (stderr_done, stderr_finished) = oneshot::channel();
        let stderr_task = AbortOnDropHandle::new(tokio::spawn(async move {
            stderr_diagnostics.drain(stderr).await;
            let _ = stderr_done.send(());
        }));
        let reader_diagnostics = diagnostics.clone();
        let (sender, messages) = mpsc::channel(2);
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(output);
            let mut decoder = super::framing::Decoder::default();
            let failure = loop {
                let mut bytes = Vec::new();
                let read = (&mut output)
                    .take((MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut bytes)
                    .await;
                if bytes.len() > MAX_FRAME {
                    break Diagnostic::new(Category::Protocol, "frame_limit");
                }
                match read {
                    Ok(0) => break Diagnostic::new(Category::Closed, "stdout"),
                    Err(_) => break Diagnostic::new(Category::Closed, "stdout_read"),
                    Ok(_) if bytes.last() != Some(&b'\n') => {
                        break Diagnostic::new(Category::Protocol, "incomplete_frame");
                    }
                    Ok(_) => {}
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    break Diagnostic::new(Category::Protocol, "invalid_json");
                };
                if !value["type"].is_string() {
                    break Diagnostic::new(Category::Protocol, "missing_frame_type");
                }
                match decoder.push(value) {
                    Ok(Some(value)) => {
                        if sender.send(value).await.is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(_) => break Diagnostic::new(Category::Protocol, "invalid_chunks"),
                }
            };
            let _ = tokio::time::timeout(Duration::from_millis(100), stderr_finished).await;
            reader_diagnostics.observe(failure, "read");
        });
        Ok(Self {
            group: child.id(),
            child,
            diagnostics,
            _stderr: stderr_task,
            input: Some(input),
            messages,
            reader,
            events: VecDeque::new(),
            buffered_bytes: 0,
            sequence: 0,
            deadline: client.deadline,
            closed: false,
        })
    }

    pub(super) async fn initialize(&mut self) -> Result<(), AgentSessionError> {
        let ready = tokio::time::timeout(self.deadline, self.messages.recv())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .ok_or(AgentSessionError::Failed)?;
        if ready["type"] != "ready"
            || !ready["supportedProtocolVersions"]
                .as_array()
                .is_some_and(|versions| versions.contains(&json!(2)))
        {
            return Err(AgentSessionError::Rejected);
        }
        let selected = self
            .request("negotiate_protocol", json!({"protocolVersion":2}))
            .await?;
        if selected["protocolVersion"] != 2 {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Rejected)?;
        if bytes.len() >= MAX_FRAME {
            return Err(AgentSessionError::Rejected);
        }
        bytes.push(b'\n');
        let input = self.input.as_mut().ok_or(AgentSessionError::Failed)?;
        if matches!(
            tokio::time::timeout(self.deadline, async {
                input.write_all(&bytes).await?;
                input.flush().await
            })
            .await,
            Ok(Ok(()))
        ) {
            return Ok(());
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    /// Send a correlated command without waiting for hooks or extension interactions.
    /// Returns bounded write failures; the caller must consume the eventual native response.
    pub(super) async fn begin(
        &mut self,
        kind: &str,
        mut fields: Value,
    ) -> Result<String, AgentSessionError> {
        if !fields.is_object() {
            return Err(AgentSessionError::Rejected);
        }
        self.sequence += 1;
        let id = format!("ait-omp-{}", self.sequence);
        fields["id"] = json!(id);
        fields["type"] = json!(kind);
        self.send(&fields).await?;
        Ok(id)
    }

    pub(super) async fn request(
        &mut self,
        kind: &str,
        fields: Value,
    ) -> Result<Value, AgentSessionError> {
        let id = self.begin(kind, fields).await?;
        let response = tokio::time::timeout(self.deadline, async {
            loop {
                let value = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                if value["type"] == "response" && value["id"] == id {
                    if value["command"] != kind {
                        return Err(AgentSessionError::Failed);
                    }
                    return match value["success"].as_bool() {
                        Some(true) => Ok(value.get("data").cloned().unwrap_or(Value::Null)),
                        Some(false) => {
                            self.diagnostics
                                .observe_native(&value["error"], "rpc", kind);
                            Err(AgentSessionError::Rejected)
                        }
                        None => Err(AgentSessionError::Failed),
                    };
                }
                self.buffered_bytes += value.to_string().len();
                if self.events.len() >= 4096 || self.buffered_bytes > MAX_BUFFER {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(value);
            }
        })
        .await;
        match response {
            Ok(Ok(data)) => Ok(data),
            Ok(Err(AgentSessionError::Rejected)) => Err(AgentSessionError::Rejected),
            result => {
                if self.diagnostics.failure().is_none() {
                    let category = if result.is_err() {
                        Category::Timeout
                    } else {
                        Category::Closed
                    };
                    self.diagnostics
                        .observe(Diagnostic::new(category, "transport"), kind);
                }
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
        }
    }

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if let Some(value) = self.events.pop_front() {
            self.buffered_bytes = self.buffered_bytes.saturating_sub(value.to_string().len());
            return Ok(Some(value));
        }
        match self.messages.try_recv() {
            Ok(value) => Ok(Some(value)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.diagnostics.flush();
        self.input.take();
        let drained = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    result = self.child.wait() => return result,
                    Some(_) = self.messages.recv() => {}
                }
            }
        })
        .await;
        self.reader.abort();
        self.kill_group();
        self.group = None;
        if matches!(drained, Ok(Ok(_))) {
            return Ok(());
        }
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(())
    }

    fn kill_group(&self) {
        #[cfg(unix)]
        if let Some(group) = self.group {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{group}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        self.kill_group();
    }
}
