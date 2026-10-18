use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use futures_util::FutureExt;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinHandle;

use crate::local::diagnostics::{self, Diagnostics};
use crate::ports::agent_session::AgentSessionError;
use crate::ports::diagnostics::{Category, Diagnostic};
use tokio_util::task::AbortOnDropHandle;

const MAX_OUTBOUND_FRAME: usize = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 128;

#[derive(Debug)]
pub(super) struct Transport {
    child: Child,
    pub(super) diagnostics: Diagnostics,
    _stderr: AbortOnDropHandle<()>,
    input: Arc<Mutex<ChildStdin>>,
    messages: mpsc::Receiver<Value>,
    reader: JoinHandle<()>,
    pending_response: Option<AbortOnDropHandle<Result<(), AgentSessionError>>>,
    events: VecDeque<Value>,
    sequence: u64,
    deadline: Duration,
    closed: bool,
    pub(super) user_agent: Option<String>,
}

impl Transport {
    pub(super) fn spawn(
        program: &Path,
        cwd: &str,
        deadline: Duration,
    ) -> Result<Self, AgentSessionError> {
        Self::spawn_with_goals(program, cwd, deadline, false)
    }

    pub(super) fn spawn_with_goals(
        program: &Path,
        cwd: &str,
        deadline: Duration,
        goals: bool,
    ) -> Result<Self, AgentSessionError> {
        Self::spawn_with_environment(
            program,
            cwd,
            deadline,
            goals,
            &crate::ports::environment::AgentEnvironment::default(),
        )
    }

    pub(super) fn spawn_with_environment(
        program: &Path,
        cwd: &str,
        deadline: Duration,
        goals: bool,
        environment: &crate::ports::environment::AgentEnvironment,
    ) -> Result<Self, AgentSessionError> {
        let mut command = Command::new(program);
        command.envs(environment.entries());
        if goals {
            command.args(["--enable", "goals"]);
        }
        command.arg("app-server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Native diagnostics may contain prompts, paths or authentication data.
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let diagnostics = Diagnostics::new("codex");
        let mut child = command
            .spawn()
            .map_err(|error| diagnostics.spawn_error(&error))?;
        let input = Arc::new(Mutex::new(
            child.stdin.take().ok_or(AgentSessionError::Failed)?,
        ));
        let output = child.stdout.take().ok_or(AgentSessionError::Failed)?;
        let stderr = child.stderr.take().ok_or(AgentSessionError::Failed)?;
        let stderr_task = AbortOnDropHandle::new(tokio::spawn(diagnostics.clone().drain(stderr)));
        let reader_diagnostics = diagnostics.clone();
        let (sender, messages) = mpsc::channel(MAX_EVENTS);
        let request_input = input.clone();
        let reader = tokio::spawn(async move {
            let mut output = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                // Paseo's Codex transport uses readline without a frame-size cap. Native
                // images and thread/read history can exceed the client request budget.
                let read = output.read_until(b'\n', &mut bytes).await;
                if !matches!(read, Ok(1..)) || bytes.last() != Some(&b'\n') {
                    let category = if matches!(read, Ok(0)) {
                        Category::Closed
                    } else {
                        Category::Protocol
                    };
                    reader_diagnostics.observe(Diagnostic::new(category, "stdout"), "read");
                    break;
                }
                let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                    reader_diagnostics
                        .observe(Diagnostic::new(Category::Protocol, "stdout"), "decode");
                    break;
                };
                if value["method"] == "mcpServer/elicitation/request"
                    && value.get("id").is_some()
                    && (!matches!(
                        value["params"]["mode"].as_str(),
                        Some("form" | "openai/form" | "openaiForm")
                    ) || crate::local::elicitation::questions(
                        &value["params"]["requestedSchema"],
                    )
                    .is_err())
                {
                    let rejection = json!({"id":value["id"],"result":{"action":"decline","content":null,"_meta":null}});
                    if !matches!(
                        tokio::time::timeout(deadline, write(&request_input, &rejection)).await,
                        Ok(Ok(()))
                    ) {
                        break;
                    }
                    continue;
                }
                if value.get("method").is_some()
                    && value.get("id").is_some()
                    && !value["method"]
                        .as_str()
                        .is_some_and(super::permissions::supported)
                {
                    // Unknown native interactions fail closed instead of becoming implicit approval.
                    let rejection = json!({"id":value["id"],"error":{
                        "code":-32601,"message":"Provider interaction is not supported"}});
                    let _ = tokio::time::timeout(deadline, write(&request_input, &rejection)).await;
                    let _ = sender
                        .send(json!({"method":"server/unsupportedRequest"}))
                        .await;
                    break;
                }
                // Retain the supported v2 stream in order; legacy mirror notifications must
                // not duplicate the same native output. The bounded channel applies backpressure.
                if value.get("id").is_none() && !retained(value["method"].as_str()) {
                    continue;
                }
                if sender.send(value).await.is_err() {
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
            pending_response: None,
            events: VecDeque::new(),
            sequence: 0,
            deadline,
            closed: false,
            user_agent: None,
        })
    }

    pub(super) async fn initialize(&mut self) -> Result<(), AgentSessionError> {
        let response = self
            .request(
                "initialize",
                json!({
                    "clientInfo":{
                        "name":"ait",
                        "title":"Ait",
                        "version":env!("CARGO_PKG_VERSION")
                    },
                    "capabilities":{"experimentalApi":true}
                }),
            )
            .await?;
        self.user_agent = response["userAgent"].as_str().map(str::to_owned);
        let result = tokio::time::timeout(
            self.deadline,
            write(&self.input, &json!({"method":"initialized","params":{}})),
        )
        .await;
        if !matches!(result, Ok(Ok(()))) {
            let _ = self.close().await;
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    pub(super) async fn request(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Value, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        self.sequence += 1;
        let request = json!({"id":self.sequence,"method":method,"params":params});
        let result = tokio::time::timeout(self.deadline, async {
            write(&self.input, &request).await?;
            loop {
                let message = self
                    .messages
                    .recv()
                    .await
                    .ok_or(AgentSessionError::Failed)?;
                if let Some(id) = message.get("id")
                    && message.get("method").is_none()
                {
                    if id.as_u64() != Some(self.sequence) {
                        return Err(AgentSessionError::Failed);
                    }
                    if let Some(error) = message.get("error") {
                        if method == "turn/steer" && super::streaming::steer_rejected(error) {
                            tracing::debug!(
                                provider = "codex",
                                operation = method,
                                "Native provider rejected steering"
                            );
                        } else {
                            self.diagnostics
                                .observe(diagnostics::native(error, "rpc"), method);
                        }
                        return Err(
                            if method != "turn/steer" || super::streaming::steer_rejected(error) {
                                AgentSessionError::Rejected
                            } else {
                                AgentSessionError::Failed
                            },
                        );
                    }
                    return message
                        .get("result")
                        .cloned()
                        .ok_or(AgentSessionError::Failed);
                }
                if self.events.len() >= MAX_EVENTS {
                    tracing::warn!(
                        queued_events = self.events.len(),
                        limit_events = MAX_EVENTS,
                        "Codex app-server pending event limit reached"
                    );
                    return Err(AgentSessionError::Failed);
                }
                self.events.push_back(message);
            }
        })
        .await;
        if let Ok(Ok(value)) = result {
            Ok(value)
        } else if matches!(result, Ok(Err(AgentSessionError::Rejected))) {
            Err(AgentSessionError::Rejected)
        } else {
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

    pub(super) async fn respond(
        &mut self,
        id: &Value,
        result: Value,
    ) -> Result<(), AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Failed);
        }
        if write_response(
            &self.input,
            &json!({"id":id,"result":result}),
            self.deadline,
        )
        .await
        .is_ok()
        {
            Ok(())
        } else {
            let _ = self.close().await;
            Err(AgentSessionError::Failed)
        }
    }

    /// Queue `result` for native request `id` from synchronous polling.
    /// Rejects closed transports or an outstanding reply; `poll` observes write failures.
    pub(super) fn queue_response(
        &mut self,
        id: &Value,
        result: Value,
    ) -> Result<(), AgentSessionError> {
        if self.closed || self.pending_response.is_some() {
            return Err(AgentSessionError::Failed);
        }
        let input = self.input.clone();
        let mut frame = json!({"id":id});
        frame["result"] = result;
        let deadline = self.deadline;
        self.pending_response = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            write_response(&input, &frame, deadline).await
        })));
        Ok(())
    }

    pub(super) fn poll(&mut self) -> Result<Option<Value>, AgentSessionError> {
        if let Some(response) = &mut self.pending_response {
            let Some(result) = response.now_or_never() else {
                return Ok(None);
            };
            self.pending_response = None;
            if !matches!(result, Ok(Ok(()))) {
                self.closed = true;
                self.diagnostics
                    .observe(Diagnostic::new(Category::Network, "transport"), "respond");
                return Err(AgentSessionError::Failed);
            }
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        match self.messages.try_recv() {
            Ok(message) if message.get("method").is_some() => Ok(Some(message)),
            Err(mpsc::error::TryRecvError::Empty) if !self.closed => Ok(None),
            Ok(_) | Err(_) => Err(AgentSessionError::Failed),
        }
    }

    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.diagnostics.flush();
        self.closed = true;
        self.pending_response = None;
        self.reader.abort();
        #[cfg(unix)]
        if let Some(id) = self.child.id() {
            let mut signal = Command::new("/bin/kill");
            signal
                .args(["-KILL", "--", &format!("-{id}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            let _ = tokio::time::timeout(Duration::from_secs(1), signal.status()).await;
        }
        // kill is idempotent for an already-exited child; wait always reaps it.
        let _ = self.child.start_kill();
        tokio::time::timeout(Duration::from_secs(2), self.child.wait())
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(())
    }
}

async fn write_response(
    input: &Mutex<ChildStdin>,
    frame: &Value,
    deadline: Duration,
) -> Result<(), AgentSessionError> {
    tokio::time::timeout(deadline, write(input, frame))
        .await
        .map_err(|_| AgentSessionError::Failed)?
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.reader.abort();
        #[cfg(unix)]
        if let Some(id) = self.child.id() {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{id}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        // kill_on_drop handles exceptional paths; normal ownership ends through close + wait.
    }
}

async fn write(input: &Mutex<ChildStdin>, value: &Value) -> Result<(), AgentSessionError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| AgentSessionError::Failed)?;
    if bytes.len() >= MAX_OUTBOUND_FRAME {
        tracing::warn!(
            size_bytes = bytes.len(),
            limit_bytes = MAX_OUTBOUND_FRAME,
            "Codex app-server outbound frame exceeds size limit"
        );
        return Err(AgentSessionError::Failed);
    }
    bytes.push(b'\n');
    let mut input = input.lock().await;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| AgentSessionError::Failed)?;
    input.flush().await.map_err(|_| AgentSessionError::Failed)
}

#[cfg(all(test, unix))]
mod tests;

fn retained(method: Option<&str>) -> bool {
    matches!(
        method,
        Some(
            "error"
                | "turn/completed"
                | "thread/started"
                | "turn/plan/updated"
                | "thread/goal/updated"
                | "thread/goal/cleared"
                | "turn/started"
                | "thread/tokenUsage/updated"
                | "serverRequest/resolved"
                | "item/completed"
                | "item/started"
                | "item/agentMessage/delta"
                | "item/reasoning/summaryTextDelta"
                | "item/commandExecution/outputDelta"
                | "item/fileChange/outputDelta"
        )
    )
}
