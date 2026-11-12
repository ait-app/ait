use std::collections::VecDeque;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::ChildStdin;
use tokio::sync::{Mutex, mpsc, oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

use super::framing::{Decoder, Frame, MAX_LINE};
use super::{LABEL, OmpClient};
use provider::local::diagnostics::Diagnostics;
use provider::local::process_group::{self, Signal};
use provider_core::ports::agent_session::AgentSessionError;
use provider_core::ports::diagnostics::{Category, Diagnostic};

/// Frames buffered while a request waits for its response.
const MAX_BUFFER: usize = 64 * 1024 * 1024;
/// Largest outbound command line. OMP 18.18.8 reads stdin lines without a size limit, so
/// this only bounds memory at the largest logical frame either side exchanges.
const MAX_OUTBOUND: usize = 64 * 1024 * 1024;
/// Reused stdout line capacity kept after an unusually large line.
const LINE_CAPACITY: usize = 64 * 1024;
/// Newest stderr bytes retained for an unexpected exit, as Paseo keeps.
const STDERR_TAIL: usize = 8 * 1024;
/// Newest whole stderr lines quoted in a failure, within the sanitized message bound.
const STDERR_EXCERPT: usize = 2 * 1024;
/// Wait after SIGTERM, and again after SIGKILL, before shutdown gives up.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Wait for an exit status after OMP closes stdout.
const EXIT_WAIT: Duration = Duration::from_secs(2);
/// Wait for stderr to finish after OMP exits; grandchildren can keep it open.
const STDERR_WAIT: Duration = Duration::from_millis(100);

/// Bounds for one native connection; the defaults follow Paseo's OMP provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Timeouts {
    /// Wait for OMP's `ready` frame after launch.
    pub(super) ready: Duration,
    /// Default wait for a response, and the bound on every stdin write.
    pub(super) request: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            ready: Duration::from_secs(20),
            request: Duration::from_secs(60),
        }
    }
}

/// How the stdout reader stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ending {
    /// OMP closed stdout between frames.
    Closed,
    /// OMP closed stdout inside a line.
    Incomplete,
    /// Reading stdout failed.
    Unreadable,
    /// A line exceeded the physical line limit.
    Oversized,
    /// The transport stopped consuming frames.
    Abandoned,
}

/// Newest stderr bytes, with whether older output was discarded.
#[derive(Debug, Default)]
struct Tail {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl Tail {
    fn push(&mut self, bytes: &[u8]) {
        let kept = &bytes[bytes.len().saturating_sub(STDERR_TAIL)..];
        self.bytes.extend(kept);
        let excess = self.bytes.len().saturating_sub(STDERR_TAIL);
        self.truncated |= excess > 0 || kept.len() < bytes.len();
        self.bytes.drain(..excess);
    }

    /// Return the newest whole lines within the excerpt budget, still unredacted.
    /// A line cut by the tail bound is omitted so redaction always sees complete lines.
    fn excerpt(&self) -> Option<String> {
        let (front, back) = self.bytes.as_slices();
        let text = String::from_utf8_lossy(&[front, back].concat()).into_owned();
        let text = if self.truncated {
            text.split_once('\n').map_or("", |(_, rest)| rest)
        } else {
            text.as_str()
        }
        .trim_end();
        let mut start = text.rfind('\n').map_or(0, |index| index + 1);
        while start > 0 {
            let previous = text[..start - 1].rfind('\n').map_or(0, |index| index + 1);
            if text.len() - previous > STDERR_EXCERPT {
                break;
            }
            start = previous;
        }
        let excerpt = text[start..].trim();
        (!excerpt.is_empty()).then(|| excerpt.to_owned())
    }
}

/// Stderr reader that copies every byte it yields into the shared tail.
struct Tee<R> {
    inner: R,
    tail: Arc<StdMutex<Tail>>,
}

impl<R: AsyncRead + Unpin> AsyncRead for Tee<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let start = buffer.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(())))
            && let Ok(mut tail) = this.tail.lock()
        {
            tail.push(&buffer.filled()[start..]);
        }
        result
    }
}

/// Explains why OMP's stdout ended unless Ait itself is shutting the process down.
#[derive(Debug)]
struct Monitor {
    diagnostics: Diagnostics,
    closing: Arc<AtomicBool>,
    exit: watch::Receiver<Option<ExitStatus>>,
    tail: Arc<StdMutex<Tail>>,
    stderr_finished: oneshot::Receiver<()>,
    group: Option<u32>,
}

impl Monitor {
    async fn settle(mut self, ending: Ending) {
        let (diagnostic, reason) = match ending {
            Ending::Abandoned => return,
            Ending::Oversized => {
                if !self.closing.load(Ordering::Acquire) {
                    self.diagnostics.observe_failure(
                        Diagnostic::new(Category::Protocol, "frame_limit"),
                        "OMP stdout exceeded the 64 MiB line limit.",
                        "read",
                    );
                    // Skipping the line would lose a response or turn event; stop the producer.
                    process_group::signal(self.group, Signal::Terminate);
                }
                return;
            }
            Ending::Closed => (Diagnostic::new(Category::Closed, "exit"), None),
            Ending::Incomplete => (
                Diagnostic::new(Category::Protocol, "incomplete_frame"),
                Some("OMP stdout ended in an incomplete JSON frame."),
            ),
            Ending::Unreadable => (
                Diagnostic::new(Category::Closed, "stdout_read"),
                Some("Ait could not read OMP stdout."),
            ),
        };
        let status = tokio::time::timeout(EXIT_WAIT, self.exit.wait_for(Option::is_some))
            .await
            .ok()
            .and_then(Result::ok)
            .and_then(|status| *status);
        let _ = tokio::time::timeout(STDERR_WAIT, &mut self.stderr_finished).await;
        if !self.closing.load(Ordering::Acquire) {
            self.explain(diagnostic, reason, status);
        }
    }

    /// Record the exit status and a bounded stderr excerpt as the connection's failure.
    /// A native explanation already retained for this turn stays authoritative, and a
    /// failure class already recognized from stderr keeps its category.
    fn explain(&self, diagnostic: Diagnostic, reason: Option<&str>, status: Option<ExitStatus>) {
        if self.diagnostics.message().is_some() {
            return;
        }
        let mut message = reason.map_or_else(String::new, |reason| format!("{reason} "));
        message.push_str(&exit_text(status));
        let excerpt = self.tail.lock().ok().and_then(|tail| tail.excerpt());
        if let Some(excerpt) = excerpt {
            message.push_str("\nstderr:\n");
            message.push_str(&excerpt);
        }
        // Diagnostics sanitizes and bounds the text, redacting URLs and credentials.
        self.diagnostics.observe_failure(
            self.diagnostics.failure().unwrap_or(diagnostic),
            &message,
            "exit",
        );
    }
}

fn exit_text(status: Option<ExitStatus>) -> String {
    #[cfg(unix)]
    let signal = status.and_then(|status| std::os::unix::process::ExitStatusExt::signal(&status));
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    match (status.and_then(|status| status.code()), signal) {
        (Some(code), _) => format!("OMP exited unexpectedly with code {code}."),
        (None, Some(signal)) => format!("OMP was stopped unexpectedly by signal {signal}."),
        (None, None) => "OMP closed its output unexpectedly.".to_owned(),
    }
}

/// Forward each decoded stdout frame until OMP closes stdout, a line exceeds `limit`
/// bytes, or the receiver goes away. Invalid lines are logged and skipped.
async fn read_frames(
    mut output: impl AsyncBufRead + Unpin,
    limit: usize,
    frames: &mpsc::Sender<Frame>,
) -> Ending {
    let mut decoder = Decoder::default();
    let mut line = Vec::new();
    let mut problems = 0_u64;
    loop {
        line.clear();
        line.shrink_to(LINE_CAPACITY);
        match (&mut output)
            .take(limit as u64 + 1)
            .read_until(b'\n', &mut line)
            .await
        {
            Ok(0) => return Ending::Closed,
            Ok(_) => {}
            Err(_) => return Ending::Unreadable,
        }
        if line.pop_if(|byte| *byte == b'\n').is_none() {
            return if line.len() > limit {
                Ending::Oversized
            } else {
                Ending::Incomplete
            };
        }
        match decoder.line(&line) {
            Ok(Some(frame)) => {
                if frames.send(frame).await.is_err() {
                    return Ending::Abandoned;
                }
            }
            Ok(None) => {}
            Err(problem) => {
                problems += 1;
                // Payloads are never logged; repeated problems are sampled.
                if problems.is_power_of_two() {
                    tracing::warn!(
                        provider = LABEL,
                        ?problem,
                        problems,
                        "Ignoring an invalid OMP frame"
                    );
                }
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct Transport {
    pub(super) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    // Owns the child; dropping it kills a process that is still running.
    watcher: Option<AbortOnDropHandle<()>>,
    exit: watch::Receiver<Option<ExitStatus>>,
    closing: Arc<AtomicBool>,
    group: Option<u32>,
    input: Option<Arc<Mutex<ChildStdin>>>,
    background_write: Option<AbortOnDropHandle<()>>,
    /// Newest queued [`Transport::reply`]; each reply waits for the one before it.
    reply_write: Option<AbortOnDropHandle<()>>,
    /// A background write failed or timed out, possibly leaving a partial line on OMP's
    /// stdin, so nothing more may be written.
    poisoned: Arc<AtomicBool>,
    messages: mpsc::Receiver<Frame>,
    reader: AbortOnDropHandle<()>,
    events: VecDeque<Frame>,
    buffered_bytes: usize,
    sequence: u64,
    timeouts: Timeouts,
    closed: bool,
}

impl Transport {
    /// Start the native RPC process in `cwd` with `args`, under `confinement` for an Agent
    /// in a sandboxed Workspace, bounding its startup and requests by `timeouts`.
    ///
    /// # Errors
    ///
    /// Returns launch, confinement or missing-pipe failures.
    pub(super) fn spawn(
        client: &OmpClient,
        cwd: &str,
        args: &[String],
        confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
        timeouts: Timeouts,
    ) -> Result<Self, AgentSessionError> {
        let diagnostics = Diagnostics::new(LABEL);
        let mut command = provider::local::sandbox::command(
            &client.program,
            confinement,
            super::sandbox::state(client),
            &|name| client.environment.variable(name),
        )?;
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
        let group = child.id();
        let tail = Arc::new(StdMutex::new(Tail::default()));
        let stderr_diagnostics = diagnostics.clone();
        let stderr_tail = Arc::clone(&tail);
        let (stderr_done, stderr_finished) = oneshot::channel();
        let stderr_task = AbortOnDropHandle::new(tokio::spawn(async move {
            let tee = Tee {
                inner: stderr,
                tail: stderr_tail,
            };
            stderr_diagnostics.drain(tee).await;
            let _ = stderr_done.send(());
        }));
        let (exited, exit) = watch::channel(None);
        let watcher = AbortOnDropHandle::new(tokio::spawn(async move {
            if let Ok(status) = child.wait().await {
                exited.send_replace(Some(status));
            }
        }));
        let closing = Arc::new(AtomicBool::new(false));
        let monitor = Monitor {
            diagnostics: diagnostics.clone(),
            closing: Arc::clone(&closing),
            exit: exit.clone(),
            tail,
            stderr_finished,
            group,
        };
        let (sender, messages) = mpsc::channel(2);
        let reader = AbortOnDropHandle::new(tokio::spawn(async move {
            let ending = read_frames(BufReader::new(output), MAX_LINE, &sender).await;
            // The sender closes only after the explanation is recorded.
            monitor.settle(ending).await;
        }));
        Ok(Self {
            diagnostics,
            _stderr: stderr_task,
            watcher: Some(watcher),
            exit,
            closing,
            group,
            input: Some(Arc::new(Mutex::new(input))),
            background_write: None,
            reply_write: None,
            poisoned: Arc::new(AtomicBool::new(false)),
            messages,
            reader,
            events: VecDeque::new(),
            buffered_bytes: 0,
            sequence: 0,
            timeouts,
            closed: false,
        })
    }

    /// The bounds this connection was started with.
    pub(super) const fn timeouts(&self) -> Timeouts {
        self.timeouts
    }

    /// Wait for OMP's `ready` frame, ignoring earlier frames, then negotiate protocol v2
    /// when OMP advertises it with the expected frame sizes; otherwise stay on v1.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` when `ready` does not arrive in time, the process
    /// closes, or OMP refuses the protocol v2 it advertised.
    pub(super) async fn initialize(&mut self) -> Result<(), AgentSessionError> {
        let ready = tokio::time::timeout(self.timeouts.ready, async {
            while let Some(frame) = self.messages.recv().await {
                if frame.value["type"] == "ready" {
                    return Some(frame.value);
                }
            }
            None
        })
        .await;
        let ready = match ready {
            Ok(Some(ready)) => ready,
            Ok(None) => return Err(AgentSessionError::Failed),
            Err(_) => {
                self.diagnostics
                    .observe(Diagnostic::new(Category::Timeout, "ready"), "ready");
                return Err(AgentSessionError::Failed);
            }
        };
        if !super::framing::supports_v2(&ready) {
            return Ok(());
        }
        let selected = self
            .request("negotiate_protocol", json!({"protocolVersion":2}))
            .await?;
        if selected["protocolVersion"] != 2 {
            self.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "negotiate_protocol"),
                "OMP did not accept the RPC protocol v2 it advertised.",
                "negotiate_protocol",
            );
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    pub(super) async fn send(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let input = self.writable()?;
        match write(input, message, self.timeouts.request).await {
            Ok(()) => return Ok(()),
            Err(AgentSessionError::Rejected) => return Err(AgentSessionError::Rejected),
            Err(_) => {}
        }
        let _ = self.close().await;
        Err(AgentSessionError::Failed)
    }

    /// Queue one read-only request of `kind`, such as `get_state`, without blocking native
    /// event or cancellation polling.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Busy` while an earlier background write is unfinished, so
    /// it is never queued twice, and `AgentSessionError::Failed` after the input closed or a
    /// background write failed. Otherwise returns the request's correlation and its write
    /// acknowledgement. A failed or timed-out write fails the next [`Transport::poll`].
    pub(super) fn background(
        &mut self,
        kind: &str,
    ) -> Result<(String, oneshot::Receiver<Result<(), AgentSessionError>>), AgentSessionError> {
        if self
            .background_write
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Err(AgentSessionError::Busy);
        }
        let input = self.writable()?.clone();
        self.sequence += 1;
        let id = format!("ait-omp-{}", self.sequence);
        let message = json!({"type":kind,"id":id});
        let limit = self.timeouts.request;
        let poisoned = Arc::clone(&self.poisoned);
        let (written, acknowledgement) = oneshot::channel();
        self.background_write = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let result = write(&input, &message, limit).await;
            if result.is_err() {
                poisoned.store(true, Ordering::Release);
            }
            let _ = written.send(result);
        })));
        Ok((id, acknowledgement))
    }

    /// Queue `message`, such as an automatic dialog answer, without blocking event polling.
    /// Replies are written in the order they were queued.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` after the input closed or a background write
    /// failed. A failed write is reported by the next [`Transport::poll`].
    pub(super) fn reply(&mut self, message: Value) -> Result<(), AgentSessionError> {
        let input = self.writable()?.clone();
        let earlier = self.reply_write.take();
        let failed = Arc::clone(&self.poisoned);
        let limit = self.timeouts.request;
        self.reply_write = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            if let Some(earlier) = earlier {
                let _ = earlier.await;
            }
            if write(&input, &message, limit).await.is_err() {
                failed.store(true, Ordering::Release);
            }
        })));
        Ok(())
    }

    /// Queue a correlated command of `kind` behind earlier replies, without blocking event
    /// polling, for work that continues from a native reply, such as `/autocompact toggle`.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Rejected` when `fields` is not an object and
    /// `AgentSessionError::Failed` after the input closed. A failed write is reported by the
    /// next [`Transport::poll`].
    pub(super) fn queue(
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
        self.reply(fields)?;
        Ok(id)
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

    /// OMP's stdin, unless it closed or a background write may have left a partial line.
    fn writable(&self) -> Result<&Arc<Mutex<ChildStdin>>, AgentSessionError> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(AgentSessionError::Failed);
        }
        self.input.as_ref().ok_or(AgentSessionError::Failed)
    }

    /// Send `kind` and wait for its response within the default request timeout.
    ///
    /// # Errors
    ///
    /// See [`Transport::request_within`].
    pub(super) async fn request(
        &mut self,
        kind: &str,
        fields: Value,
    ) -> Result<Value, AgentSessionError> {
        self.request_within(kind, fields, Some(self.timeouts.request))
            .await
    }

    /// Send `kind` and wait for its correlated response, buffering other frames for polling.
    /// `timeout` of `None` waits until the response arrives or the connection closes, for
    /// long-running commands such as `get_available_models`.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Rejected` when OMP refuses the command. Any other failure,
    /// including an elapsed `timeout`, closes the connection and returns
    /// `AgentSessionError::Failed`.
    pub(super) async fn request_within(
        &mut self,
        kind: &str,
        fields: Value,
        timeout: Option<Duration>,
    ) -> Result<Value, AgentSessionError> {
        let id = self.begin(kind, fields).await?;
        let response = match timeout {
            Some(limit) => tokio::time::timeout(limit, self.response(&id, kind)).await,
            None => Ok(self.response(&id, kind).await),
        };
        self.settle(kind, response).await
    }

    /// Send `kind` and wait up to `limit` for its correlated response, for an optional read
    /// whose absence the caller tolerates. Returns `None` when `limit` elapses; the connection
    /// stays open and the late response arrives as an uncorrelated event.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Rejected` when OMP refuses the command. Any other failure
    /// closes the connection and returns `AgentSessionError::Failed`.
    pub(super) async fn request_unless_late(
        &mut self,
        kind: &str,
        fields: Value,
        limit: Duration,
    ) -> Result<Option<Value>, AgentSessionError> {
        let id = self.begin(kind, fields).await?;
        match tokio::time::timeout(limit, self.response(&id, kind)).await {
            Err(_) => Ok(None),
            response => self.settle(kind, response).await.map(Some),
        }
    }

    /// Return a response's data, or close the connection after any failure but a refusal.
    async fn settle(
        &mut self,
        kind: &str,
        response: Result<Result<Value, AgentSessionError>, tokio::time::error::Elapsed>,
    ) -> Result<Value, AgentSessionError> {
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

    async fn response(&mut self, id: &str, kind: &str) -> Result<Value, AgentSessionError> {
        loop {
            let frame = self
                .messages
                .recv()
                .await
                .ok_or(AgentSessionError::Failed)?;
            let value = &frame.value;
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
            // Uncorrelated responses are events too; the session decides what they mean.
            self.buffered_bytes += frame.bytes;
            if self.events.len() >= 4096 || self.buffered_bytes > MAX_BUFFER {
                return Err(AgentSessionError::Failed);
            }
            self.events.push_back(frame);
        }
    }

    /// Whether the OMP process has exited or this connection was closed.
    pub(super) fn exited(&self) -> bool {
        self.closed || self.exit.borrow().is_some()
    }

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if self.poisoned.load(Ordering::Acquire) && !self.closed {
            self.diagnostics.observe_failure(
                Diagnostic::new(Category::Closed, "write"),
                "Ait could not finish writing to OMP, whose input may now hold a partial \
                 command; the connection cannot be used.",
                "write",
            );
            return Err(AgentSessionError::Failed);
        }
        if let Some(frame) = self.events.pop_front() {
            self.buffered_bytes -= frame.bytes;
            return Ok(Some(frame.value));
        }
        match self.messages.try_recv() {
            Ok(frame) => Ok(Some(frame.value)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Err(_) => Err(AgentSessionError::Failed),
        }
    }

    /// End stdin, then stop the process group with SIGTERM, escalating to SIGKILL after
    /// [`SHUTDOWN_GRACE`]. Leftover group members are always killed.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` when OMP has not exited after SIGKILL.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.closing.store(true, Ordering::Release);
        self.diagnostics.flush();
        self.background_write.take();
        self.reply_write.take();
        self.input.take();
        let group = self.group.take();
        process_group::signal(group, Signal::Terminate);
        let graceful = self.wait_for_exit().await;
        process_group::signal(group, Signal::Kill);
        self.reader.abort();
        if graceful {
            return Ok(());
        }
        if group.is_none() {
            // Without a process group, dropping the owner kills the direct child.
            self.watcher.take();
            return Ok(());
        }
        if self.wait_for_exit().await {
            Ok(())
        } else {
            Err(AgentSessionError::Failed)
        }
    }

    /// Wait up to [`SHUTDOWN_GRACE`] for the process to exit, draining stdout meanwhile.
    async fn wait_for_exit(&mut self) -> bool {
        tokio::time::timeout(SHUTDOWN_GRACE, async {
            loop {
                tokio::select! {
                    result = self.exit.wait_for(Option::is_some) => return result.is_ok(),
                    Some(_) = self.messages.recv() => {}
                }
            }
        })
        .await
        .unwrap_or(false)
    }
}

async fn write(
    input: &Mutex<ChildStdin>,
    message: &Value,
    limit: Duration,
) -> Result<(), AgentSessionError> {
    let mut bytes = serde_json::to_vec(message).map_err(|_| AgentSessionError::Rejected)?;
    if bytes.len() > MAX_OUTBOUND {
        return Err(AgentSessionError::Rejected);
    }
    bytes.push(b'\n');
    if matches!(
        tokio::time::timeout(limit, async {
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
        self.closing.store(true, Ordering::Release);
        process_group::signal(self.group, Signal::Kill);
    }
}

#[cfg(test)]
mod tests;
