use std::collections::VecDeque;
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, OnceCell, mpsc, oneshot};
use tokio::task::JoinHandle;

use super::{PROVIDER, PiClient};
use crate::local::diagnostics::Diagnostics;
use crate::ports::{
    agent_session::AgentSessionError,
    diagnostics::{Category, Diagnostic},
};

const MAX_VERSION_BYTES: usize = 64;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
const VERSION_RETRY_INTERVAL: Duration = Duration::from_secs(5);

const MAX_FRAME: usize = 64 * 1024 * 1024;
const MAX_PENDING: usize = 128;
// Large native records fail explicitly with a frame_limit protocol diagnostic.
const MAX_INBOUND_FRAME: usize = 2 * 1024 * 1024;
const MAX_PENDING_BYTES: usize = 8 * 1024 * 1024;

/// Validated version string, cached for one configured client and its clones.
#[derive(Debug)]
pub(super) struct Version {
    /// Sanitized version for actionable installation diagnostics.
    pub(super) text: String,
    /// Whether this version implements the required settlement protocol.
    pub(super) supported: bool,
}

/// Safe version-probe failures, retaining spawn facts without paths or native error text.
#[derive(Debug, Clone, Copy, thiserror::Error)]
pub(super) enum VersionError {
    /// The OS rejected launching the configured executable.
    #[error(
        "Pi executable could not be started (error kind: {kind:?}{os_error}); check its interpreter, executable format, and execution permissions.",
        os_error = .os_error.map_or_else(String::new, |code| format!(", OS error: {code}"))
    )]
    Spawn {
        /// Safe OS error classification.
        kind: std::io::ErrorKind,
        /// Native numeric error code when provided by the OS.
        os_error: Option<i32>,
    },
    /// The probe timed out or failed to return a valid version successfully.
    #[error("Pi version probe failed or timed out; requires Pi 1.2 or newer.")]
    Failed,
}

/// Shared successful version, retry cooldown, and diagnostics for one configured client.
#[derive(Debug)]
pub(super) struct VersionProbe {
    value: OnceCell<Version>,
    failure: Mutex<Option<(VersionError, tokio::time::Instant)>>,
    diagnostics: Diagnostics,
}

impl Default for VersionProbe {
    fn default() -> Self {
        Self {
            value: OnceCell::new(),
            failure: Mutex::new(None),
            diagnostics: Diagnostics::new(PROVIDER),
        }
    }
}

/// Cache successful versions and serialize retries after a short failure cooldown.
pub(super) async fn version_info(client: &PiClient) -> Result<&Version, VersionError> {
    if let Some(version) = client.version.value.get() {
        return Ok(version);
    }
    let mut failure = client.version.failure.lock().await;
    if let Some((error, at)) = *failure
        && at.elapsed() < VERSION_RETRY_INTERVAL
    {
        return Err(error);
    }
    let result = client
        .version
        .value
        .get_or_try_init(|| probe_version(client))
        .await;
    *failure = result
        .as_ref()
        .err()
        .map(|error| (*error, tokio::time::Instant::now()));
    result
}

/// Require native settlement support, returning unavailable for an old installation.
pub(super) async fn version(client: &PiClient) -> Result<(), AgentSessionError> {
    let version = version_info(client).await.map_err(|error| match error {
        VersionError::Spawn { .. } => AgentSessionError::Unavailable,
        VersionError::Failed => AgentSessionError::Failed,
    })?;
    if version.supported {
        Ok(())
    } else {
        Err(AgentSessionError::Unavailable)
    }
}

async fn probe_version(client: &PiClient) -> Result<Version, VersionError> {
    let diagnostics = &client.version.diagnostics;
    let mut child = Command::new(&client.program)
        .arg("--version")
        .envs(client.environment.entries())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            diagnostics.spawn_error(&error);
            VersionError::Spawn {
                kind: error.kind(),
                os_error: error.raw_os_error(),
            }
        })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or(VersionError::Failed)?
        .take((MAX_VERSION_BYTES + 1) as u64);
    let result = tokio::time::timeout(VERSION_TIMEOUT.min(client.deadline), async {
        let mut bytes = Vec::new();
        stdout
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        let version = parse_version(&bytes)?;
        if !child
            .wait()
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .success()
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(version)
    })
    .await;
    let category = if result.is_err() {
        Category::Timeout
    } else {
        Category::Protocol
    };
    let result = result.unwrap_or(Err(AgentSessionError::Failed));
    if result.is_err() {
        diagnostics.observe(Diagnostic::new(category, "version"), "version");
        let _ = child.kill().await;
    }
    result.map_err(|_| VersionError::Failed)
}

fn parse_version(bytes: &[u8]) -> Result<Version, AgentSessionError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| AgentSessionError::Failed)?
        .trim();
    if bytes.len() > MAX_VERSION_BYTES {
        return Err(AgentSessionError::Failed);
    }
    if !text
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
    {
        return Err(AgentSessionError::Failed);
    }
    let core = text.split(['-', '+']).next().unwrap_or_default();
    let numbers: Option<Vec<u32>> = core.split('.').map(|part| part.parse().ok()).collect();
    let numbers = numbers.ok_or(AgentSessionError::Failed)?;
    // Numeric short/extended formats remain actionable instead of becoming probe failures.
    let supported = numbers.len() == 3 && (numbers[0] > 1 || numbers[0] == 1 && numbers[1] >= 2);
    Ok(Version {
        text: text.to_owned(),
        supported,
    })
}

#[derive(Debug)]
struct Record {
    value: Value,
    bytes: usize,
}

/// Owned JSONL process with bounded queues, request correlation and shutdown.
#[derive(Debug)]
pub(super) struct Transport {
    child: Option<Child>,
    input: Arc<Mutex<Option<ChildStdin>>>,
    messages: mpsc::Receiver<Record>,
    reader: JoinHandle<()>,
    stderr: JoinHandle<()>,
    pending: VecDeque<Record>,
    pending_bytes: usize,
    sequence: u64,
    deadline: Duration,
    pub(super) admission_deadline: Duration,
    closed: bool,
    group: Option<u32>,
    /// Safe bounded diagnostics shared with process readers and the session.
    pub(super) diagnostics: Diagnostics,
}

impl Transport {
    /// Start the configured executable in `cwd` with additional native `args`.
    /// Returns unavailable on launch failure or failed for missing process pipes.
    pub(super) fn spawn(
        client: &PiClient,
        cwd: &str,
        args: &[OsString],
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = Diagnostics::new(PROVIDER);
        let mut command = Command::new(&client.program);
        command
            .args(["--mode", "rpc"])
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
        let input = Arc::new(Mutex::new(child.stdin.take()));
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let errors = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let stderr_diagnostics = diagnostics.clone();
        let (stderr_done, stderr_finished) = oneshot::channel();
        let stderr = tokio::spawn(async move {
            stderr_diagnostics.drain(errors).await;
            let _ = stderr_done.send(());
        });
        let (sender, messages) = mpsc::channel(MAX_PENDING);
        let writer = input.clone();
        let deadline = client.deadline;
        let reader_diagnostics = diagnostics.clone();
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(output);
            let failure = loop {
                let mut bytes = Vec::new();
                let read = (&mut output)
                    .take((MAX_INBOUND_FRAME + 1) as u64)
                    .read_until(b'\n', &mut bytes)
                    .await;
                if bytes.len() > MAX_INBOUND_FRAME {
                    break Diagnostic::new(Category::Protocol, "frame_limit");
                }
                if matches!(read, Ok(0)) {
                    break Diagnostic::new(Category::Closed, "stdout");
                }
                if !matches!(read, Ok(1..)) || bytes.last() != Some(&b'\n') {
                    break Diagnostic::new(Category::Protocol, "frame");
                }
                let Ok(message) = serde_json::from_slice::<Value>(&bytes) else {
                    break Diagnostic::new(Category::Protocol, "decode");
                };
                if !message["type"].is_string() {
                    break Diagnostic::new(Category::Protocol, "record");
                }
                if message["type"] == "extension_ui_request" {
                    if let Err(failure) = extension_response(&writer, &message, deadline).await {
                        break failure;
                    }
                    continue;
                }
                if sender
                    .send(Record {
                        value: message,
                        bytes: bytes.len(),
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            };
            // Allow final stderr classification to arrive before publishing stream closure.
            let _ = tokio::time::timeout(Duration::from_millis(100), stderr_finished).await;
            reader_diagnostics.observe(failure, "read");
        });
        Ok(Self {
            group: child.id(),
            child: Some(child),
            input,
            messages,
            reader,
            stderr,
            pending: VecDeque::new(),
            pending_bytes: 0,
            sequence: 0,
            deadline,
            admission_deadline: client.admission_deadline,
            closed: false,
            diagnostics,
        })
    }

    /// Write one record once; uncertain writes close the transport and return failure.
    async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        if let Err(category) = write(&self.input, message, self.deadline).await {
            self.diagnostics
                .observe(Diagnostic::new(category, "write"), "write");
            let _ = self.close().await;
            return Err(if category == Category::Protocol {
                AgentSessionError::Rejected
            } else {
                AgentSessionError::Failed
            });
        }
        Ok(())
    }

    /// Correlate a control response while retaining bounded intervening records.
    pub(super) async fn request(&mut self, message: Value) -> Result<Value, AgentSessionError> {
        self.request_with(message, self.deadline, |_| Ok(())).await
    }

    /// Submit a prompt with a distinct admission budget, consuming events as they arrive.
    /// Returns native rejection or an uncertain timeout/protocol failure without replay.
    pub(super) async fn prompt(
        &mut self,
        message: Value,
        consume: impl FnMut(&Value) -> Result<(), Diagnostic>,
    ) -> Result<Value, AgentSessionError> {
        self.request_with(message, self.admission_deadline, consume)
            .await
    }

    async fn request_with(
        &mut self,
        mut message: Value,
        deadline: Duration,
        mut consume: impl FnMut(&Value) -> Result<(), Diagnostic>,
    ) -> Result<Value, AgentSessionError> {
        let prompt = message["type"] == "prompt";
        self.sequence += 1;
        let id = format!("ait-pi-{}", self.sequence);
        message["id"] = json!(id);
        self.send(&message).await?;
        let result = tokio::time::timeout(deadline, async {
            loop {
                let record = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                let response = &record.value;
                if response["type"] == "response" {
                    if response["id"] != id || response["command"] != message["type"] {
                        return Err(AgentSessionError::Failed);
                    }
                    return match response["success"].as_bool() {
                        Some(true) => Ok(response["data"].clone()),
                        Some(false) => {
                            self.diagnostics
                                .observe_native(&response["error"], "rpc", "request");
                            Err(AgentSessionError::Rejected)
                        }
                        None => Err(AgentSessionError::Failed),
                    };
                }
                if prompt {
                    if let Err(diagnostic) = consume(response) {
                        self.diagnostics.observe(diagnostic, "prompt");
                        return Err(AgentSessionError::Failed);
                    }
                } else {
                    self.pending_bytes = self.pending_bytes.saturating_add(record.bytes);
                    if self.pending.len() >= MAX_PENDING || self.pending_bytes > MAX_PENDING_BYTES {
                        return Err(AgentSessionError::Failed);
                    }
                    self.pending.push_back(record);
                }
            }
        })
        .await;
        if result.is_err() {
            self.diagnostics.observe(
                Diagnostic::new(
                    Category::Timeout,
                    if prompt {
                        "prompt_admission"
                    } else {
                        "control_response"
                    },
                ),
                "request",
            );
        }
        let result = result.unwrap_or(Err(AgentSessionError::Failed));
        if matches!(result, Err(AgentSessionError::Failed)) {
            self.diagnostics
                .observe(Diagnostic::new(Category::Protocol, "response"), "request");
            let _ = self.close().await;
        }
        result
    }

    fn pending(&mut self) -> Option<Value> {
        let record = self.pending.pop_front()?;
        self.pending_bytes -= record.bytes;
        Some(record.value)
    }

    /// Receive a queued native event, failing on closed streams.
    pub(super) async fn receive(&mut self) -> Result<Value, AgentSessionError> {
        if let Some(message) = self.pending() {
            return Ok(message);
        }
        self.messages
            .recv()
            .await
            .map(|record| record.value)
            .ok_or(AgentSessionError::Failed)
    }

    /// Read one event without waiting; closed streams return failure.
    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if let Some(message) = self.pending() {
            return Ok(Some(message));
        }
        match self.messages.try_recv() {
            Ok(record) => Ok(Some(record.value)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    /// Drain only events preceding the most recent control response, providing an ordering barrier.
    pub(super) fn drain_pending(&mut self) -> impl Iterator<Item = Value> + '_ {
        std::iter::from_fn(|| self.pending())
    }

    /// Allow native cleanup, then force termination if needed and reap the owned process.
    /// Returns failure if shutdown cannot reap the process.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.reader.abort();
        if let Some(child) = self.child.take() {
            shutdown(child, self.group.take(), self.input.clone())
                .await
                .map_err(|_| AgentSessionError::Failed)??;
        }
        self.input.lock().await.take();
        if tokio::time::timeout(Duration::from_millis(100), &mut self.stderr)
            .await
            .is_err()
        {
            self.stderr.abort();
        }
        self.diagnostics.flush();
        Ok(())
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        self.stderr.abort();
        if let Some(child) = self.child.take() {
            // The worker owns the process until it is reaped, even without a live runtime.
            drop(shutdown(child, self.group.take(), self.input.clone()));
        }
    }
}

async fn extension_response(
    input: &Mutex<Option<ChildStdin>>,
    message: &Value,
    deadline: Duration,
) -> Result<(), Diagnostic> {
    match message["method"].as_str() {
        // Pi 1.2 emits IDs for these fire-and-forget UI notifications as well as dialogs.
        Some("notify" | "setStatus" | "setWidget" | "setTitle" | "set_editor_text") => Ok(()),
        Some("select" | "confirm" | "input" | "editor") => {
            if !message["id"].is_string() {
                return Err(Diagnostic::new(Category::Protocol, "extension_id"));
            }
            let response =
                json!({"type":"extension_ui_response","id":message["id"],"cancelled":true});
            write(input, &response, deadline)
                .await
                .map_err(|category| Diagnostic::new(category, "extension_response"))
        }
        _ => Err(Diagnostic::new(Category::Protocol, "extension_method")),
    }
}

async fn write(
    input: &Mutex<Option<ChildStdin>>,
    message: &Value,
    deadline: Duration,
) -> Result<(), Category> {
    let mut bytes = serde_json::to_vec(message).map_err(|_| Category::Protocol)?;
    if bytes.len() >= MAX_FRAME {
        return Err(Category::Protocol);
    }
    bytes.push(b'\n');
    tokio::time::timeout(deadline, async {
        let mut guard = input.lock().await;
        let writer = guard.as_mut().ok_or(Category::Closed)?;
        writer
            .write_all(&bytes)
            .await
            .map_err(|_| Category::Closed)?;
        writer.flush().await.map_err(|_| Category::Closed)
    })
    .await
    .map_err(|_| Category::Timeout)?
}

#[cfg(test)]
mod tests;

/// Give native signal handlers time to terminate detached tools before forcing shutdown.
fn shutdown(
    mut child: Child,
    group: Option<u32>,
    input: Arc<Mutex<Option<ChildStdin>>>,
) -> oneshot::Receiver<Result<(), AgentSessionError>> {
    let (finished, completion) = oneshot::channel();
    // A Tokio task can be cancelled during runtime shutdown, invoking kill_on_drop too early.
    std::thread::spawn(move || {
        signal_group(group, "-TERM");
        // Keep stdin alive on Unix: EOF can exit Pi before its signal handler cleans up tools.
        #[cfg(not(unix))]
        if let Ok(mut input) = input.try_lock() {
            input.take();
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut forced = false;
        let result = loop {
            match child.try_wait() {
                Ok(Some(_)) => break Ok(()),
                Err(_) => break Err(AgentSessionError::Failed),
                Ok(None) => {}
            }
            if !forced && Instant::now() >= deadline {
                signal_group(group, "-KILL");
                if child.start_kill().is_err() {
                    break Err(AgentSessionError::Failed);
                }
                forced = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        drop(input);
        let _ = finished.send(result);
    });
    completion
}

/// Signal an owned process group, ignoring an already-exited group.
fn signal_group(group: Option<u32>, signal: &str) {
    #[cfg(unix)]
    if let Some(group) = group {
        let _ = std::process::Command::new("/bin/kill")
            .args([signal, "--", &format!("-{group}")])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(unix))]
    let _ = (group, signal);
}
