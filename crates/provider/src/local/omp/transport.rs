use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::task::AbortOnDropHandle;

use super::{LABEL, OmpClient};
use crate::local::diagnostics::Diagnostics;
use crate::local::process_group::{self, Signal};
use crate::ports::agent_session::AgentSessionError;
use crate::ports::diagnostics::{Category, Diagnostic};

const MAX_FRAME: usize = 1024 * 1024;
const MAX_BUFFER: usize = 64 * 1024 * 1024;
/// Cold native process/catalog initialization budget, independent of connected controls.
pub(super) const STARTUP_TIMEOUT: Duration = Duration::from_mins(3);

/// Decoded native frame with the wire bytes it occupied, counted once against buffer limits.
#[derive(Debug)]
struct Record {
    value: Value,
    bytes: usize,
}

/// Cloneable access to the same serialized native stdin, used by detached host-tool receipts.
#[derive(Debug, Clone)]
pub(super) struct Writer {
    input: Arc<Mutex<ChildStdin>>,
    deadline: Duration,
}

impl Writer {
    /// Write one bounded frame without creating another process or another session writer.
    pub(super) async fn send(&self, frame: &Value) -> Result<(), AgentSessionError> {
        write(&self.input, frame, self.deadline).await
    }
}

#[derive(Debug)]
pub(super) struct Transport {
    child: Child,
    pub(super) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    group: Option<u32>,
    input: Option<Arc<Mutex<ChildStdin>>>,
    background_write: Option<AbortOnDropHandle<()>>,
    messages: mpsc::Receiver<Record>,
    reader: JoinHandle<()>,
    events: VecDeque<Record>,
    buffered_bytes: usize,
    sequence: u64,
    deadline: Duration,
    closed: bool,
    command_completion: bool,
    completion_support: Arc<RwLock<Option<bool>>>,
    /// Latest native host greeting, refreshed from [`Self::capability_state`] on demand.
    pub(super) capability: Option<super::vibe::Capability>,
    capability_state: Arc<RwLock<Option<super::vibe::Capability>>>,
    /// Bounded `ait_vibe_state`/`ait_vibe_worker` outbox filled by the reader.
    vibe_state: Arc<RwLock<Vec<Value>>>,
    configuration: Option<tempfile::NamedTempFile>,
    extension: Option<tempfile::NamedTempFile>,
    startup: Option<mpsc::Sender<super::bootstrap::Frame>>,
}

impl Transport {
    /// Start the native RPC process in `cwd` with `args`, under `confinement` for an Agent
    /// in a sandboxed Workspace.
    /// Returns launch, confinement or missing-pipe failures.
    pub(super) fn spawn(
        client: &OmpClient,
        cwd: &str,
        args: &[String],
        confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = Diagnostics::new(LABEL);
        let mut command = crate::local::sandbox::command(
            &client.program,
            confinement,
            crate::local::sandbox::OMP_STATE,
            &|name| client.environment.variable(name),
        )?;
        command
            .args(["--cwd", cwd])
            .args(args)
            .current_dir(cwd)
            .envs(client.environment.entries())
            .env("PI_RPC_EMIT_TITLE", "1")
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
        let capability_state: Arc<RwLock<Option<super::vibe::Capability>>> = Arc::default();
        let reader_capability = capability_state.clone();
        let vibe_state: Arc<RwLock<Vec<Value>>> = Arc::default();
        let reader_vibe = vibe_state.clone();
        let (sender, messages) = mpsc::channel(2);
        let reader = tokio::spawn(async move {
            let failure = read_frames(output, &sender, &reader_capability, &reader_vibe).await;
            let _ = tokio::time::timeout(Duration::from_millis(100), stderr_finished).await;
            reader_diagnostics.observe(failure, "read");
        });
        Ok(Self {
            group: child.id(),
            child,
            diagnostics,
            _stderr: stderr_task,
            input: Some(Arc::new(Mutex::new(input))),
            background_write: None,
            messages,
            reader,
            events: VecDeque::new(),
            buffered_bytes: 0,
            sequence: 0,
            deadline: client.deadline,
            closed: false,
            command_completion: false,
            completion_support: client.completion_support.clone(),
            capability: None,
            capability_state,
            vibe_state,
            configuration: None,
            extension: None,
            startup: None,
        })
    }

    /// Retain the private role overlay until this native owner has exited.
    pub(super) fn retain_configuration(&mut self, configuration: Option<tempfile::NamedTempFile>) {
        self.configuration = configuration;
    }

    /// Keep a public extension available for native reloads until this owner has closed.
    pub(super) fn retain_extension(&mut self, extension: Option<tempfile::NamedTempFile>) {
        self.extension = extension;
    }

    /// Borrow the connection's existing serialized input for an asynchronous host-tool result.
    pub(super) fn writer(&self) -> Result<Writer, AgentSessionError> {
        Ok(Writer {
            input: self
                .input
                .as_ref()
                .ok_or(AgentSessionError::Failed)?
                .clone(),
            deadline: self.deadline,
        })
    }

    /// Publish startup UI through the owning Ait session while native initialization is pending.
    /// Returns an unavailable error when the bootstrap receiver has gone away.
    pub(super) async fn startup(
        &mut self,
        sender: mpsc::Sender<super::bootstrap::Frame>,
    ) -> Result<(), AgentSessionError> {
        sender
            .send(super::bootstrap::Frame::Writer(self.writer()?))
            .await
            .map_err(|_| AgentSessionError::Unavailable)?;
        self.startup = Some(sender);
        Ok(())
    }

    /// Finish startup routing once the live session can consume and answer its own UI events.
    pub(super) fn finish_startup(&mut self) {
        self.startup.take();
    }

    pub(super) async fn initialize(&mut self) -> Result<(), AgentSessionError> {
        let ready = tokio::time::timeout(STARTUP_TIMEOUT.max(self.deadline), self.messages.recv())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .ok_or(AgentSessionError::Failed)?
            .value;
        if ready["type"] != "ready"
            || !ready["supportedProtocolVersions"]
                .as_array()
                .is_some_and(|versions| versions.contains(&json!(2)))
        {
            return Err(AgentSessionError::Rejected);
        }
        let control_deadline = self.deadline;
        self.deadline = STARTUP_TIMEOUT.max(control_deadline);
        let selected = self
            .request("negotiate_protocol", json!({"protocolVersion":2}))
            .await;
        self.deadline = control_deadline;
        let selected = selected?;
        if selected["protocolVersion"] != 2 {
            return Err(AgentSessionError::Failed);
        }
        self.command_completion = ready["capabilities"]["commandCompletion"]["version"] == 1;
        *self
            .completion_support
            .write()
            .map_err(|_| AgentSessionError::Failed)? = Some(self.command_completion);
        Ok(())
    }

    /// Native feature negotiated from the official runtime's ready frame.
    pub(super) fn command_completion(&self) -> bool {
        self.command_completion
    }

    /// Snapshot the native host greeting captured by the reader, if one has arrived.
    pub(super) fn refresh_capability(&mut self) -> Option<&super::vibe::Capability> {
        self.capability = self
            .capability_state
            .read()
            .ok()
            .and_then(|state| state.clone());
        self.capability.as_ref()
    }

    /// Take the adapter frames the reader captured since the last call, in arrival order.
    pub(super) fn take_vibe_frames(&mut self) -> Vec<Value> {
        self.vibe_state
            .write()
            .map(|mut frames| std::mem::take(&mut *frames))
            .unwrap_or_default()
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let input = self.input.as_ref().ok_or(AgentSessionError::Failed)?;
        match write(input, message, self.deadline).await {
            Ok(()) => return Ok(()),
            Err(AgentSessionError::Rejected) => return Err(AgentSessionError::Rejected),
            Err(_) => {}
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    /// Queue one read-only state request without blocking native event or cancellation polling.
    /// Returns its correlation and write acknowledgement; a busy writer is not queued twice.
    pub(super) fn check_state(
        &mut self,
    ) -> Result<(String, oneshot::Receiver<Result<(), AgentSessionError>>), AgentSessionError> {
        if self
            .background_write
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Err(AgentSessionError::Busy);
        }
        let input = self
            .input
            .as_ref()
            .ok_or(AgentSessionError::Failed)?
            .clone();
        self.sequence += 1;
        let id = format!("ait-omp-{}", self.sequence);
        let message = json!({"type":"get_state","id":id});
        let deadline = self.deadline;
        let (written, acknowledgement) = oneshot::channel();
        self.background_write = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = written.send(write(&input, &message, deadline).await);
        })));
        Ok((id, acknowledgement))
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
                let record = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                let value = &record.value;
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
                if value["type"] == "extension_ui_request"
                    && let Some(sender) = &self.startup
                {
                    sender
                        .send(super::bootstrap::Frame::Ui(value.clone()))
                        .await
                        .map_err(|_| AgentSessionError::Unavailable)?;
                    if matches!(
                        value["method"].as_str(),
                        Some("select" | "confirm" | "input" | "editor" | "ask" | "cancel")
                    ) {
                        continue;
                    }
                }
                self.buffered_bytes += record.bytes;
                if self.events.len() >= 4096 || self.buffered_bytes > MAX_BUFFER {
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(record);
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
        if let Some(record) = self.events.pop_front() {
            self.buffered_bytes -= record.bytes;
            return Ok(Some(record.value));
        }
        match self.messages.try_recv() {
            Ok(record) => Ok(Some(record.value)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    /// Remove buffered observations superseded by an acknowledged snapshot on this ordered stream.
    /// Unrelated turn and interaction frames keep their order and byte accounting.
    pub(super) fn supersede_buffered(&mut self, kind: &str) {
        self.events.retain(|record| {
            if record.value["type"] == kind {
                self.buffered_bytes -= record.bytes;
                false
            } else {
                true
            }
        });
    }

    /// Restore unrelated frames consumed while a local control ran on its existing writer.
    /// Returns a bounded-buffer failure before changing the queue; original order is retained.
    pub(super) fn restore_events(&mut self, events: Vec<Value>) -> Result<(), AgentSessionError> {
        let records: Vec<_> = events
            .into_iter()
            .map(|value| {
                let bytes = value.to_string().len();
                Record { value, bytes }
            })
            .collect();
        let bytes = records
            .iter()
            .try_fold(self.buffered_bytes, |total, record| {
                total.checked_add(record.bytes)
            })
            .filter(|bytes| *bytes <= MAX_BUFFER)
            .ok_or(AgentSessionError::Failed)?;
        for record in records.into_iter().rev() {
            self.events.push_front(record);
        }
        self.buffered_bytes = bytes;
        Ok(())
    }

    /// Execute a known local command on an isolated metadata reader, awaiting its final receipt.
    /// Admission alone is insufficient for extension handlers. Agent work, rejection, oversized
    /// output and missing completion remain errors. Owner controls must restore unrelated returned frames.
    pub(super) async fn local_command(
        &mut self,
        message: &str,
    ) -> Result<Vec<Value>, AgentSessionError> {
        let id = self.begin("prompt", json!({"message":message})).await?;
        tokio::time::timeout(self.deadline, async {
            let mut admitted = false;
            let mut completed = false;
            let mut events = Vec::new();
            let mut bytes = 0;
            while !admitted || !completed {
                let event = match self.poll()? {
                    Some(event) => event,
                    None => {
                        self.messages
                            .recv()
                            .await
                            .ok_or(AgentSessionError::Failed)?
                            .value
                    }
                };
                if event["type"] == "response" && event["id"] == id {
                    if event["command"] != "prompt" || event["success"] != true {
                        return Err(AgentSessionError::Rejected);
                    }
                    if event["data"]["agentInvoked"] == true {
                        return Err(AgentSessionError::Rejected);
                    }
                    admitted = true;
                    completed |= event["data"]["agentInvoked"] == false;
                } else if event["type"] == "prompt_result" && event["id"] == id {
                    if event["agentInvoked"] != false || event["status"] != "completed" {
                        return Err(AgentSessionError::Rejected);
                    }
                    completed = true;
                } else {
                    bytes += event.to_string().len();
                    if events.len() >= 4096 || bytes > MAX_BUFFER {
                        return Err(AgentSessionError::Failed);
                    }
                    events.push(event);
                }
            }
            Ok(events)
        })
        .await
        .map_err(|_| AgentSessionError::Failed)?
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.diagnostics.flush();
        self.background_write.take();
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
        process_group::signal(self.group, Signal::Kill);
        self.group = None;
        if matches!(drained, Ok(Ok(_))) {
            self.configuration.take();
            return Ok(());
        }
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        self.configuration.take();
        self.extension.take();
        Ok(())
    }
}

/// Read newline-delimited native frames until a protocol or pipe failure, routing the
/// external bridge greeting and Vibe frames away from the public session stream.
async fn read_frames(
    output: tokio::process::ChildStdout,
    sender: &mpsc::Sender<Record>,
    capability: &RwLock<Option<super::vibe::Capability>>,
    vibe: &RwLock<Vec<Value>>,
) -> Diagnostic {
    let mut output = BufReader::new(output);
    let mut decoder = super::framing::Decoder::default();
    let mut frame_bytes = 0;
    loop {
        let mut bytes = Vec::new();
        let read = (&mut output)
            .take((MAX_FRAME + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .await;
        if bytes.len() > MAX_FRAME {
            return Diagnostic::new(Category::Protocol, "frame_limit");
        }
        match read {
            Ok(0) => return Diagnostic::new(Category::Closed, "stdout"),
            Err(_) => return Diagnostic::new(Category::Closed, "stdout_read"),
            Ok(_) if bytes.last() != Some(&b'\n') => {
                return Diagnostic::new(Category::Protocol, "incomplete_frame");
            }
            Ok(_) => {}
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return Diagnostic::new(Category::Protocol, "invalid_json");
        };
        if !value["type"].is_string() {
            return Diagnostic::new(Category::Protocol, "missing_frame_type");
        }
        frame_bytes += bytes.len();
        match decoder.push(value) {
            Ok(Some(value)) => {
                let bytes = std::mem::take(&mut frame_bytes);
                if let Some(greeting) = super::vibe::hello(&value) {
                    if let Ok(mut state) = capability.write() {
                        *state = Some(greeting);
                    }
                    continue;
                }
                if super::vibe::is_frame(&value) {
                    if let Ok(mut frames) = vibe.write() {
                        super::vibe::store(&mut frames, &value);
                    }
                    continue;
                }
                if sender.send(Record { value, bytes }).await.is_err() {
                    return Diagnostic::new(Category::Closed, "session_reader");
                }
            }
            Ok(None) => {}
            Err(_) => return Diagnostic::new(Category::Protocol, "invalid_chunks"),
        }
    }
}

async fn write(
    input: &Mutex<ChildStdin>,
    message: &Value,
    deadline: Duration,
) -> Result<(), AgentSessionError> {
    let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Rejected)?;
    if bytes.len() >= MAX_FRAME {
        return Err(AgentSessionError::Rejected);
    }
    bytes.push(b'\n');
    if matches!(
        tokio::time::timeout(deadline, async {
            let mut input = input.lock().await;
            input.write_all(&bytes).await?;
            input.flush().await
        })
        .await,
        Ok(Ok(()))
    ) {
        Ok(())
    } else {
        Err(AgentSessionError::Failed)
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        process_group::signal(self.group, Signal::Kill);
    }
}

#[cfg(all(test, unix))]
mod tests;
