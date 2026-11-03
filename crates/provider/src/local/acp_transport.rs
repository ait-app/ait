use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;

use crate::local::diagnostics::Diagnostics;
use crate::ports::agent_session::AgentSessionError;
use crate::ports::diagnostics::{Category, Diagnostic};

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_EVENTS: usize = 128;

/// Owned, bounded JSON-RPC stdio connection.
#[derive(Debug)]
pub(crate) struct Transport {
    child: Child,
    pub(crate) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    input: Option<ChildStdin>,
    group: Option<u32>,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    events: VecDeque<Value>,
    sequence: u64,
    deadline: Duration,
    closed: bool,
}

impl Transport {
    /// Launch a configured command in an owned process group; returns child or pipe failures.
    pub(crate) fn spawn(
        mut command: tokio::process::Command,
        cwd: &str,
        deadline: Duration,
        provider: &'static str,
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = Diagnostics::new(provider);
        command
            .current_dir(cwd)
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
        let stderr_task = AbortOnDropHandle::new(tokio::spawn(diagnostics.clone().drain(stderr)));
        let reader_diagnostics = diagnostics.clone();
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
                    let category = if matches!(read, Ok(0)) {
                        Category::Closed
                    } else {
                        Category::Protocol
                    };
                    reader_diagnostics.observe(Diagnostic::new(category, "stdout"), "read");
                    break;
                }
                if bytes.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else {
                    reader_diagnostics
                        .observe(Diagnostic::new(Category::Protocol, "stdout"), "decode");
                    break;
                };
                if message["jsonrpc"] != "2.0" || sender.send(message).await.is_err() {
                    break;
                }
            }
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
            sequence: 0,
            deadline,
            closed: false,
        })
    }

    /// Return the configured deadline for one control operation, including its confirmation.
    /// Prompt execution uses unbounded reads and does not use this duration.
    pub(crate) const fn control_deadline(&self) -> Duration {
        self.deadline
    }

    /// Set the deadline for subsequent control requests on this owned connection.
    pub(crate) fn set_control_deadline(&mut self, deadline: Duration) {
        self.deadline = deadline;
    }

    /// Allow one potentially slow native initialization request, then restore normal controls.
    pub(crate) async fn startup_request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AgentSessionError> {
        let ordinary_timeout = self.deadline;
        self.deadline = timeout;
        let result = self.request(method, params).await;
        self.deadline = ordinary_timeout;
        result
    }

    /// Write and flush one bounded frame; closes uncertain transports on write failure.
    pub(crate) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Failed)?;
        if bytes.len() >= 64 * 1024 * 1024 {
            return Err(AgentSessionError::Rejected);
        }
        bytes.push(b'\n');
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

    /// Submit exactly one request and return its correlation ID without awaiting its response.
    pub(crate) async fn begin(
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

    /// Await a control response, retaining notifications and rejecting unsupported callbacks.
    pub(crate) async fn request(
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
                    if let Some(error) = message.get("error") {
                        self.diagnostics.observe_native(error, "rpc", method);
                    }
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
            result => {
                let category = if result.is_err() {
                    Category::Timeout
                } else {
                    Category::Network
                };
                if self.diagnostics.failure().is_none() {
                    self.diagnostics
                        .observe(Diagnostic::new(category, "transport"), method);
                }
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
        }
    }

    /// Consume replay notifications as they arrive rather than buffering a complete transcript.
    /// Returns protocol, callback, timeout or child-process failures without retrying the request.
    pub(crate) async fn request_with_updates(
        &mut self,
        method: &str,
        params: Value,
        mut update: impl FnMut(&Value) -> Result<(), AgentSessionError> + Send,
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
                    if let Some(error) = message.get("error") {
                        self.diagnostics.observe_native(error, "rpc", method);
                    }
                    return response(&message);
                }
                if message.get("id").is_some() {
                    self.send(&json!({"jsonrpc":"2.0","id":message["id"],
                        "error":{"code":-32601,"message":"Unsupported client method"}}))
                        .await?;
                } else {
                    update(&message)?;
                }
            }
        })
        .await;
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(AgentSessionError::Rejected)) => Err(AgentSessionError::Rejected),
            result => {
                let category = if result.is_err() {
                    Category::Timeout
                } else {
                    Category::Network
                };
                if self.diagnostics.failure().is_none() {
                    self.diagnostics
                        .observe(Diagnostic::new(category, "transport"), method);
                }
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
        }
    }

    /// Read one queued frame without blocking; returns failure after child disconnection.
    pub(crate) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if let Some(message) = self.events.pop_front() {
            return Ok(Some(message));
        }
        match self.messages.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    /// Wait for one notification or response, preserving configuration notification order.
    /// Returns a failure when the child closes its protocol stream.
    pub(crate) async fn receive(&mut self) -> Result<Value, AgentSessionError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        self.messages.recv().await.ok_or(AgentSessionError::Failed)
    }

    /// Stop the reader and reap the owned process group within a bounded deadline.
    pub(crate) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.diagnostics.flush();
        self.closed = true;
        self.input.take();
        let started = tokio::time::Instant::now();
        let mut killed_at = None;
        // Process exit can lag signal delivery under load. Reuse the connection's control
        // budget, bounded to 2–10 seconds, rather than failing an acknowledged close at 2s.
        let reap_budget = self
            .deadline
            .clamp(Duration::from_secs(2), Duration::from_secs(10));
        // Observe exit before deadlines: a delayed runtime poll must not turn an already
        // reaped peer into a timeout. The force budget begins when SIGKILL is actually sent.
        let result = loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break Ok(()),
                Err(_) => break Err(AgentSessionError::Failed),
                Ok(None) => {}
            }
            if killed_at.is_none() && started.elapsed() >= Duration::from_millis(100) {
                kill_group(self.group);
                let _ = self.child.start_kill();
                killed_at = Some(tokio::time::Instant::now());
            }
            if killed_at.is_some_and(|at| at.elapsed() >= reap_budget) {
                break Err(AgentSessionError::Failed);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        self.reader.abort();
        kill_group(self.group);
        self.group = None;
        result
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        kill_group(self.group);
    }
}

fn kill_group(group: Option<u32>) {
    #[cfg(unix)]
    if let Some(id) = group.and_then(|id| i32::try_from(id).ok()) {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(id),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(not(unix))]
    let _ = group;
}

/// Decode a JSON-RPC result or return a safe rejection/failure without exposing native payloads.
pub(crate) fn response(message: &Value) -> Result<Value, AgentSessionError> {
    if message.get("result").is_some() && message.get("error").is_none() {
        return Ok(message["result"].clone());
    }
    if message["error"]["code"].as_i64().is_some() && message.get("result").is_none() {
        return Err(AgentSessionError::Rejected);
    }
    Err(AgentSessionError::Failed)
}

#[cfg(all(test, unix))]
mod tests;
