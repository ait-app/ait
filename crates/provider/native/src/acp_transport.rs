use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;

use crate::diagnostics::Diagnostics;
use crate::process_group::{self, Signal};
use provider_core::ports::agent_session::AgentSessionError;
use provider_core::ports::diagnostics::{Category, Diagnostic};

const MAX_FRAME: usize = 8 * 1024 * 1024;
const MAX_EVENTS: usize = 128;

/// Owned, bounded JSON-RPC stdio connection.
#[derive(Debug)]
pub struct Transport {
    child: Child,
    /// Bounded stderr explanations of the native process, shared with the owning adapter.
    pub diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    input: ChildStdin,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    events: VecDeque<Value>,
    sequence: u64,
    deadline: Duration,
    closed: bool,
}

impl Transport {
    /// Launch a configured command in an owned process group; returns child or pipe failures.
    ///
    /// # Errors
    ///
    /// Returns the diagnosed spawn failure, or [`AgentSessionError::Failed`] when a standard pipe
    /// is missing.
    pub fn spawn(
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
                // Without a newline, only a short successful read proves EOF. Reaching the
                // `take` limit may leave unread bytes and must remain a protocol failure.
                let trailing = matches!(read, Ok(1..))
                    && bytes.len() < MAX_FRAME
                    && bytes.last() != Some(&b'\n');
                if !matches!(read, Ok(1..)) || (bytes.last() != Some(&b'\n') && !trailing) {
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
                if message["jsonrpc"] != "2.0" {
                    reader_diagnostics
                        .observe(Diagnostic::new(Category::Protocol, "stdout"), "decode");
                    break;
                }
                if sender.send(message).await.is_err() {
                    break;
                }
                if trailing {
                    reader_diagnostics.observe(Diagnostic::new(Category::Closed, "stdout"), "read");
                    break;
                }
            }
        });
        Ok(Self {
            child,
            diagnostics,
            _stderr: stderr_task,
            input,
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
    #[must_use]
    pub const fn control_deadline(&self) -> Duration {
        self.deadline
    }

    /// Set the deadline for subsequent control requests on this owned connection.
    pub fn set_control_deadline(&mut self, deadline: Duration) {
        self.deadline = deadline;
    }

    /// Allow one potentially slow native initialization request, then restore normal controls.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::request`].
    pub async fn startup_request(
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
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Rejected`] for a frame of 64 MiB or more and
    /// [`AgentSessionError::Failed`] when the transport is closed or the write fails.
    pub async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
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

    /// Submit exactly one request and return its correlation ID without awaiting its response.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::send`].
    pub async fn begin(
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
    ///
    /// # Errors
    ///
    /// Returns the native rejection, or [`AgentSessionError::Failed`] on a protocol, timeout or
    /// child-process failure, which also closes the transport.
    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, AgentSessionError> {
        let mut events = std::mem::take(&mut self.events);
        let result = self
            .exchange(method, params, |message| {
                if events.len() >= MAX_EVENTS {
                    return Err(AgentSessionError::Failed);
                }
                events.push_back(message);
                Ok(())
            })
            .await;
        self.events = events;
        result
    }

    /// Consume replay notifications as they arrive rather than buffering a complete transcript.
    /// Frames other than the response and native callbacks, which are rejected, go to `update`.
    /// Returns protocol, callback, timeout or child-process failures without retrying the request.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::request`], and any error from `update`.
    pub async fn request_with_updates(
        &mut self,
        method: &str,
        params: Value,
        mut update: impl FnMut(&Value) -> Result<(), AgentSessionError> + Send,
    ) -> Result<Value, AgentSessionError> {
        self.exchange(method, params, |message| update(&message))
            .await
    }

    /// Submit one request and await its response under the control deadline.
    ///
    /// Native callbacks are rejected so a provider cannot deadlock waiting for an answer while
    /// no prompt runs; every other frame goes to `other` in arrival order. Failures other than
    /// a native rejection close the connection.
    async fn exchange(
        &mut self,
        method: &str,
        params: Value,
        mut other: impl FnMut(Value) -> Result<(), AgentSessionError> + Send,
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
                if message.get("id").is_some() && message["method"].is_string() {
                    self.send(&json!({"jsonrpc":"2.0","id":message["id"],
                        "error":{"code":-32601,"message":"Unsupported client method"}}))
                        .await?;
                    continue;
                }
                other(message)?;
            }
        })
        .await;
        match self
            .diagnostics
            .settle(result, method, &[AgentSessionError::Rejected])
        {
            Err(AgentSessionError::Failed) => {
                let _ = self.close().await;
                Err(AgentSessionError::Failed)
            }
            settled => settled,
        }
    }

    /// Read one queued frame without blocking; returns failure after child disconnection.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Failed`] once the child has disconnected.
    pub fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
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
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Failed`] once the child has closed its protocol stream.
    pub async fn receive(&mut self) -> Result<Value, AgentSessionError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        self.messages.recv().await.ok_or(AgentSessionError::Failed)
    }

    /// Stop the reader and reap the owned process group within a bounded deadline.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Failed`] when the child is not reaped within two seconds.
    pub async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.diagnostics.flush();
        self.closed = true;
        self.reader.abort();
        process_group::signal(self.child.id(), Signal::Kill);
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
        process_group::signal(self.child.id(), Signal::Kill);
    }
}

/// Decode a JSON-RPC result or return a safe rejection/failure without exposing native payloads.
///
/// # Errors
///
/// Returns [`AgentSessionError::Rejected`] for a native rejection and
/// [`AgentSessionError::Failed`] for any other error or a missing result.
pub fn response(message: &Value) -> Result<Value, AgentSessionError> {
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
