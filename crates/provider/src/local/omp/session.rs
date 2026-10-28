use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

use super::{
    OmpClient, config, history, interactions::Dialogs, stream::Stream, transport::Transport,
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::protocol::prompt::AgentPrompt;

const STATE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const CANCEL_GRACE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnPhase {
    Idle,
    AwaitingAdmission,
    Running,
    AwaitingSettlement,
}

#[derive(Debug)]
struct StateCheck {
    id: String,
    // New work relinquishes ownership while retaining correlation for the delayed reply.
    turn: Option<String>,
    written: Option<oneshot::Receiver<Result<(), AgentSessionError>>>,
}

#[derive(Debug, Default)]
struct NativeBridge {
    enabled: bool,
    controls: BTreeSet<String>,
    events: std::collections::VecDeque<Value>,
    callbacks: BTreeMap<String, String>,
    buffered_bytes: usize,
}

#[derive(Debug)]
pub(super) struct Session {
    client: OmpClient,
    transport: Transport,
    cwd: String,
    state: Value,
    config: StoredAgentConfig,
    service_tiers: config::ServiceTiers,
    stream: Stream,
    dialogs: Dialogs,
    active: Option<String>,
    closed: bool,
    children: super::subagents::Children,
    prompt_id: Option<String>,
    // An earlier prompt's uncorrelated session_settled can follow the next admission.
    phase: TurnPhase,
    state_check: Option<StateCheck>,
    next_state_check: Instant,
    idle_without_outcome: Option<Instant>,
    cancellation_started: Option<Instant>,
    cancel_requests: BTreeMap<String, String>,
    input_requests: BTreeMap<String, String>,
    submitted: bool,
    created_at: String,
    native_bridge: NativeBridge,
}

/// Start a disposable native capability reader for `model` in `cwd`.
/// Returns invalid configuration, startup failures, or a mismatched native model selection.
pub(super) async fn probe(
    client: &OmpClient,
    cwd: &str,
    model: Option<&str>,
) -> Result<Transport, AgentSessionError> {
    config::validate_spec(&AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig::default(),
    })?;
    let mut args = vec![
        "--mode".to_owned(),
        "rpc-ui".to_owned(),
        "--no-session".to_owned(),
    ];
    let route = model.map(config::model_route).transpose()?;
    if let Some([provider, model]) = &route {
        args.extend([
            "--provider".to_owned(),
            provider.clone(),
            "--model".to_owned(),
            model.clone(),
        ]);
    }
    let mut transport = Transport::spawn(client, cwd, &args)?;
    transport.initialize().await?;
    let state = transport.request("get_state", json!({})).await?;
    if route.is_some_and(|[provider, model]| {
        state["model"]["provider"] != provider || state["model"]["id"] != model
    }) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(transport)
}

async fn connect(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    path: Option<&str>,
) -> Result<(Transport, Value, config::ServiceTiers), AgentSessionError> {
    let mut args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(path) = path {
        args.extend(["--resume".to_owned(), path.to_owned()]);
    }
    if let Some(prompt) = &spec.config.system_prompt {
        args.extend(["--append-system-prompt".to_owned(), prompt.clone()]);
    }
    if let Some(mode) = &spec.config.mode_id {
        args.extend([
            "--approval-mode".to_owned(),
            config::approval_mode(mode).to_owned(),
        ]);
    }
    let mut transport = Transport::spawn(client, &spec.cwd, &args)?;
    transport.initialize().await?;
    let state = config::apply(&mut transport, &spec.config).await?;
    transport
        .request("set_ask_dialog", json!({"enabled":true}))
        .await?;
    transport
        .request("set_subagent_subscription", json!({"level":"events"}))
        .await?;
    let runtime = config::runtime(&state)?;
    let tiers = client
        .service_tiers(&spec.cwd, runtime.model.as_deref())
        .await?;
    Ok((transport, state, tiers))
}

pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    let journal = if let Some(handle) = handle {
        Some(history::load(client, handle, &spec.cwd).await?)
    } else {
        None
    };
    let path = journal
        .as_ref()
        .map(|journal| journal.path.to_str().ok_or(AgentSessionError::Failed))
        .transpose()?;
    let persisted = path.is_some_and(|path| std::path::Path::new(path).is_file());
    let created_at = journal
        .as_ref()
        .and_then(|journal| journal.header["timestamp"].as_str())
        .map_or_else(|| chrono::Utc::now().to_rfc3339(), str::to_owned);
    let (transport, state, service_tiers) =
        connect(client, spec, path.filter(|_| persisted)).await?;
    let id = config::text(&state, "sessionId")?;
    if persisted && handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    let mut stream = match (journal, handle) {
        (Some(journal), Some(handle)) if persisted => journal.project(client, handle)?.1,
        _ => Stream::new(id.to_owned(), client.images.clone()),
    };
    stream = stream.with_blobs(history::root(client)?.with_file_name("blobs"));
    stream.set_context(&state);
    Ok(Session {
        client: client.clone(),
        transport,
        cwd: spec.cwd.clone(),
        state,
        config: spec.config.clone(),
        service_tiers,
        stream,
        dialogs: Dialogs::default(),
        active: None,
        closed: false,
        children: super::subagents::Children::default()
            .with_blobs(history::root(client)?.with_file_name("blobs")),
        prompt_id: None,
        phase: TurnPhase::Idle,
        state_check: None,
        next_state_check: Instant::now(),
        idle_without_outcome: None,
        cancellation_started: None,
        cancel_requests: BTreeMap::new(),
        input_requests: BTreeMap::new(),
        submitted: false,
        created_at,
        native_bridge: NativeBridge::default(),
    })
}

fn validate_native_control(kind: &str) -> Result<(), AgentSessionError> {
    if matches!(
        kind,
        "prompt"
            | "steer"
            | "follow_up"
            | "abort"
            | "abort_and_prompt"
            | "abort_and_restore_queue"
            | "new_session"
            | "switch_session"
            | "open_session"
            | "branch"
            | "fork"
            | "handoff"
            | "negotiate_protocol"
    ) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

impl Session {
    /// Apply native settings while idle, retaining the persisted session when reopening.
    /// Invalid settings or an active turn reject before any native mutation.
    pub(super) async fn configure(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        if self.closed || self.active.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        if self.config == *config {
            return Ok(());
        }
        if self.config.mode_id == config.mode_id
            && self.config.system_prompt == config.system_prompt
        {
            self.state = config::apply(&mut self.transport, config).await?;
            let runtime = config::runtime(&self.state)?;
            self.service_tiers = self
                .client
                .service_tiers(&self.cwd, runtime.model.as_deref())
                .await?;
            self.stream.set_context(&self.state);
            self.config.clone_from(config);
        } else {
            self.transport.close().await?;
            let path = config::text(&self.state, "sessionFile")?.to_owned();
            let spec = AgentSessionSpec {
                provider: "omp".to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
            };
            let persisted = std::path::Path::new(&path).is_file();
            let (transport, state, service_tiers) =
                connect(&self.client, &spec, persisted.then_some(path.as_str())).await?;
            if persisted && state["sessionId"] != self.state["sessionId"] {
                return Err(AgentSessionError::Failed);
            }
            if !persisted {
                self.stream = Stream::new(
                    config::text(&state, "sessionId")?.to_owned(),
                    self.client.images.clone(),
                )
                .with_blobs(history::root(&self.client)?.with_file_name("blobs"));
            }
            self.transport = transport;
            self.state_check = None;
            self.cancel_requests.clear();
            self.input_requests.clear();
            self.idle_without_outcome = None;
            self.state = state;
            self.service_tiers = service_tiers;
            self.stream.set_context(&self.state);
            self.config.clone_from(config);
        }
        Ok(())
    }

    /// Forward a native control command without permitting a second conversation owner.
    /// Lifecycle commands must use the ACP methods that maintain turn correlation.
    pub(super) async fn native_request(
        &mut self,
        kind: &str,
        fields: Value,
    ) -> Result<Value, AgentSessionError> {
        validate_native_control(kind)?;
        self.transport.request(kind, fields).await
    }

    /// Submit a native extension control without blocking callback delivery or polling.
    /// Correlation and pending count are bounded; lifecycle commands remain owned by ACP.
    pub(super) async fn begin_native_control(
        &mut self,
        kind: &str,
        fields: Value,
    ) -> Result<String, AgentSessionError> {
        validate_native_control(kind)?;
        if !self.native_bridge.enabled || self.native_bridge.controls.len() >= 128 {
            return Err(AgentSessionError::Rejected);
        }
        let id = self.transport.begin(kind, fields).await?;
        self.native_bridge.controls.insert(id.clone());
        Ok(id)
    }

    /// Enable a negotiated native event channel without changing OMP or its plugin loading.
    pub(super) fn forward_native_events(&mut self, enabled: bool) {
        self.native_bridge.enabled = enabled;
    }

    /// Answer a currently owned native callback, preserving all plugin-defined payload fields.
    /// Rejects stale identities and conversation commands; partial host tool updates stay pending.
    pub(super) async fn native_response(&mut self, frame: &Value) -> Result<(), AgentSessionError> {
        let id = config::text(frame, "id")?;
        let kind = config::text(frame, "type")?;
        let owner = self
            .native_bridge
            .callbacks
            .get(id)
            .ok_or(AgentSessionError::Rejected)?;
        let allowed = match owner.as_str() {
            "host_tool_call" => matches!(kind, "host_tool_update" | "host_tool_result"),
            "host_uri_request" => kind == "host_uri_result",
            "extension_ui_request" => kind == "extension_ui_response",
            _ => false,
        };
        if !allowed {
            return Err(AgentSessionError::Rejected);
        }
        self.transport.send(frame).await?;
        if kind != "host_tool_update" {
            self.native_bridge.callbacks.remove(id);
            self.dialogs.remove(id);
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
        }
        Ok(())
    }

    /// Drain bounded, uninterpreted native extension UI frames for negotiated ACP clients.
    pub(super) fn extension_frames(&mut self) -> Vec<Value> {
        self.native_bridge.buffered_bytes = 0;
        self.native_bridge.events.drain(..).collect()
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        config::validate(config)?;
        if self.closed || self.active.is_some() || prompt.output_schema.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        self.configure(config).await?;
        let blocks = prompt.blocks()?;
        let text = blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let images: Vec<_> = blocks
            .into_iter()
            .filter(|block| block["type"] == "image")
            .collect();
        self.transport.diagnostics.clear();
        self.stream
            .pending_inputs
            .push_back(prompt.client_message_id.clone());
        let prompt_id = match self
            .transport
            .begin("prompt", json!({"message":text,"images":images}))
            .await
        {
            Ok(id) => id,
            Err(error) => {
                self.stream.pending_inputs.pop_back();
                return Err(error);
            }
        };
        let turn = Uuid::new_v4().to_string();
        self.stream.begin(turn.clone());
        self.active = Some(turn.clone());
        self.prompt_id = Some(prompt_id);
        self.phase = TurnPhase::AwaitingAdmission;
        self.cancellation_started = None;
        self.next_state_check = Instant::now() + STATE_CHECK_INTERVAL;
        self.idle_without_outcome = None;
        Ok(turn)
    }

    fn observe_native_event(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if self.native_bridge.enabled {
            if self.native_bridge.events.len() >= 128 {
                return Err(AgentSessionError::Failed);
            }
            self.native_bridge.buffered_bytes = self
                .native_bridge
                .buffered_bytes
                .saturating_add(event.to_string().len());
            if self.native_bridge.buffered_bytes > 64 * 1024 * 1024 {
                return Err(AgentSessionError::Failed);
            }
            self.native_bridge
                .events
                .push_back(super::bridge::redact_event(event));
            if matches!(
                event["type"].as_str(),
                Some("host_tool_call" | "host_uri_request")
            ) || (event["type"] == "extension_ui_request"
                && !matches!(
                    event["method"].as_str(),
                    Some(
                        "notify"
                            | "setStatus"
                            | "setWidget"
                            | "setTitle"
                            | "set_editor_text"
                            | "cancel"
                    )
                ))
            {
                if self.native_bridge.callbacks.len() >= 128 {
                    return Err(AgentSessionError::Failed);
                }
                self.native_bridge.callbacks.insert(
                    config::text(event, "id")?.to_owned(),
                    config::text(event, "type")?.to_owned(),
                );
            }
            if matches!(
                event["type"].as_str(),
                Some("host_tool_cancel" | "host_uri_cancel")
            ) || (event["type"] == "extension_ui_request" && event["method"] == "cancel")
            {
                self.native_bridge
                    .callbacks
                    .remove(event["targetId"].as_str().unwrap_or_default());
            }
        }
        Ok(())
    }

    fn prompt_admitted(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if event["command"] != "prompt" {
            return Err(AgentSessionError::Failed);
        }
        if event["success"] == false {
            self.stream.pending_inputs.pop_back();
            self.transport
                .diagnostics
                .observe_native(&event["error"], "rpc", "prompt");
            self.stream.failed = true;
            self.finish();
        } else if event["success"] == true {
            self.phase = TurnPhase::Running;
            if event["data"]["agentInvoked"] == false {
                self.stream.pending_inputs.pop_back();
                self.finish();
            }
        } else {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    fn control_response(&mut self, event: &Value) -> Result<bool, AgentSessionError> {
        if self
            .state_check
            .as_ref()
            .is_some_and(|check| event["id"] == check.id)
        {
            self.checked_state(event)?;
            return Ok(true);
        }
        if let Some(id) = event["id"].as_str()
            && let Some(turn) = self.input_requests.remove(id)
        {
            if self.active.as_deref() != Some(&turn) {
                return Ok(true);
            }
            if event["command"] != "steer" || event["success"] != true {
                self.transport
                    .diagnostics
                    .observe_native(&event["error"], "rpc", "steer");
                return Err(AgentSessionError::Failed);
            }
            return Ok(true);
        }
        if let Some(id) = event["id"].as_str()
            && let Some(turn) = self.cancel_requests.remove(id)
        {
            if self.active.as_deref() != Some(&turn) {
                return Ok(true);
            }
            if event["command"] != "abort_and_restore_queue" || event["success"] != true {
                self.transport
                    .diagnostics
                    .observe_native(&event["error"], "rpc", "abort");
                return Err(AgentSessionError::Failed);
            }
            self.stream.aborted = true;
            self.stream.pending_inputs.clear();
            self.phase = TurnPhase::AwaitingSettlement;
            self.next_state_check = Instant::now();
            return Ok(true);
        }
        Ok(false)
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if event["type"] == "response" && self.control_response(event)? {
            return Ok(());
        }
        self.observe_native_event(event)?;
        if event["type"] == "agent_start" {
            self.invalidate_state_check();
        }
        if event["type"] == "message_end"
            && matches!(event["message"]["role"].as_str(), Some("assistant"))
        {
            self.submitted = true;
        }
        if event["type"] == "message_end" && event["message"]["role"] == "assistant" {
            if event["message"]["stopReason"] == "error" {
                self.transport.diagnostics.observe_native(
                    &event["message"]["errorMessage"],
                    "message_end",
                    "turn",
                );
            } else {
                self.transport.diagnostics.clear();
            }
        }
        match event["type"].as_str() {
            Some("extension_ui_request") => {
                let events = self.dialogs.capture(event)?;
                if events.is_empty() {
                    self.project(event)?;
                }
                self.stream.events.extend(events);
            }
            Some("agent_start") if self.active.is_none() => {
                let turn = Uuid::new_v4().to_string();
                self.stream.begin(turn.clone());
                self.active = Some(turn.clone());
                self.phase = TurnPhase::AwaitingSettlement;
                self.next_state_check = Instant::now() + STATE_CHECK_INTERVAL;
                self.stream.events.push_back(AgentTurnEvent::Started(turn));
            }
            Some("subagent_lifecycle" | "subagent_progress" | "subagent_event") => {
                self.stream.events.extend(self.children.consume(
                    event,
                    config::text(&self.state, "sessionId")?,
                    &self.cwd,
                    &self.client.images,
                )?);
            }
            Some("response") if self.prompt_id.as_deref() == event["id"].as_str() => {
                self.prompt_admitted(event)?;
            }
            Some("prompt_result") if self.prompt_id.as_deref() == event["id"].as_str() => {
                self.stream.failed = event["status"] == "error";
                if self.stream.failed {
                    self.transport
                        .diagnostics
                        .observe_native(&event["error"], "result", "turn");
                }
                self.stream.aborted = event["status"] == "aborted";
                self.phase = TurnPhase::AwaitingSettlement;
                if event["sessionSettled"] == true {
                    self.finish();
                }
            }
            Some("session_settled") if self.phase == TurnPhase::AwaitingSettlement => self.finish(),
            Some("rpc_frame_error") => {
                let error = if event["error"].is_object() {
                    &event["error"]
                } else {
                    event
                };
                self.transport
                    .diagnostics
                    .observe_native(error, "rpc_frame_error", "turn");
                return Err(AgentSessionError::Failed);
            }
            Some("host_tool_call" | "host_uri_request") if self.native_bridge.enabled => {}
            Some("host_tool_call" | "host_uri_request") => {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "host_request"),
                    "OMP requested a host tool or URI handler that Ait did not register.",
                    "turn",
                );
                return Err(AgentSessionError::Failed);
            }
            Some("response")
                if event["id"]
                    .as_str()
                    .is_some_and(|id| self.native_bridge.controls.remove(id)) => {}
            Some("response") if event["success"] == false => {
                self.transport
                    .diagnostics
                    .observe_native(&event["error"], "rpc", "turn");
                return Err(AgentSessionError::Failed);
            }
            _ => self.project(event)?,
        }
        Ok(())
    }

    fn project(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        self.stream.consume(event).map_err(|error| {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "event"),
                &error.to_string(),
                "project",
            );
            AgentSessionError::Failed
        })
    }

    fn finish(&mut self) {
        if self.active.take().is_some() {
            self.prompt_id = None;
            self.native_bridge.callbacks.clear();
            self.phase = TurnPhase::Idle;
            self.cancellation_started = None;
            self.idle_without_outcome = None;
            self.stream.events.extend(self.dialogs.clear());
            self.stream.events.push_back(if self.stream.aborted {
                AgentTurnEvent::Cancelled
            } else if self.stream.failed {
                AgentTurnEvent::Failed
            } else {
                AgentTurnEvent::Completed(self.stream.last_text.clone())
            });
        }
    }

    fn invalidate_state_check(&mut self) {
        if let Some(check) = &mut self.state_check {
            check.turn = None;
        }
        self.idle_without_outcome = None;
    }

    fn checked_state(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let check = self.state_check.take().ok_or(AgentSessionError::Failed)?;
        self.next_state_check = Instant::now() + STATE_CHECK_INTERVAL;
        if check.turn.is_none() || self.active != check.turn {
            return Ok(());
        }
        if event["command"] != "get_state" {
            return Err(AgentSessionError::Failed);
        }
        // Older native profiles may omit settlement facts; absence never means idle.
        let state = &event["data"];
        if event["success"] != true || state["isSettled"] != true {
            self.idle_without_outcome = None;
            return Ok(());
        }
        if state["sessionId"] != self.state["sessionId"] {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "state_session"),
                "OMP returned settlement for a different native session.",
                "get_state",
            );
            return Err(AgentSessionError::Failed);
        }
        if state["isStreaming"] == true
            || state["isCompacting"] == true
            || state["hasPendingAsyncWork"] == true
            || state["queuedMessageCount"]
                .as_u64()
                .is_some_and(|count| count > 0)
        {
            self.idle_without_outcome = None;
            return Ok(());
        }
        if self.cancellation_started.is_some() {
            self.stream.aborted = true;
            self.stream.pending_inputs.clear();
        } else if self.phase != TurnPhase::AwaitingSettlement {
            // A state response can beat the correlated result on the native event loop.
            let idle_since = self.idle_without_outcome.get_or_insert_with(Instant::now);
            if idle_since.elapsed() < STATE_CHECK_INTERVAL {
                return Ok(());
            }
            self.stream.failed = true;
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "missing_prompt_result"),
                "OMP became idle without reporting this prompt's outcome.",
                "get_state",
            );
        }
        self.finish();
        Ok(())
    }

    fn check_cancellation(&self) -> Result<(), AgentSessionError> {
        if self
            .cancellation_started
            .is_some_and(|started| started.elapsed() >= CANCEL_GRACE.min(self.client.deadline))
        {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Timeout, "abort_confirmation"),
                "OMP did not confirm cancellation; its native process will be closed.",
                "abort",
            );
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    fn poll_state_check(&mut self) -> Result<(), AgentSessionError> {
        if let Some(check) = &mut self.state_check
            && let Some(written) = &mut check.written
        {
            match written.try_recv() {
                Ok(Ok(())) => check.written = None,
                Err(oneshot::error::TryRecvError::Empty) => {}
                Ok(Err(_)) | Err(oneshot::error::TryRecvError::Closed) => {
                    self.transport.diagnostics.observe_failure(
                        Diagnostic::new(Category::Closed, "state_write"),
                        "Ait could not write an OMP state check.",
                        "get_state",
                    );
                    return Err(AgentSessionError::Failed);
                }
            }
        }
        let Some(turn) = self.active.as_ref().filter(|_| {
            matches!(
                self.phase,
                TurnPhase::Running | TurnPhase::AwaitingSettlement
            )
        }) else {
            return Ok(());
        };
        if self.state_check.is_some() || Instant::now() < self.next_state_check {
            return Ok(());
        }
        let (id, written) = match self.transport.check_state() {
            Ok(check) => check,
            Err(AgentSessionError::Busy) => return Ok(()),
            Err(error) => return Err(error),
        };
        self.state_check = Some(StateCheck {
            id,
            turn: Some(turn.clone()),
            written: Some(written),
        });
        Ok(())
    }
}

impl AgentSession for Session {
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        Ok(())
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        let tiers = if config.model == self.config.model {
            self.service_tiers
        } else {
            config::ServiceTiers::default()
        };
        Some(json!({"availableModes":config::modes(),"features":config::features(config, tiers)}))
    }

    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.transport.diagnostics.failure()
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        if let Some(message) = self.transport.diagnostics.message() {
            return Some(Cow::Owned(message));
        }
        let diagnostic = self.transport.diagnostics.failure()?;
        let reason = match diagnostic.source {
            "frame_limit" => "OMP stdout exceeded the 1 MiB frame limit.",
            "incomplete_frame" => "OMP stdout ended in an incomplete JSON frame.",
            "invalid_json" => "OMP stdout contained invalid JSON.",
            "missing_frame_type" => "OMP stdout contained a frame without a string type.",
            "invalid_chunks" => "OMP returned invalid or out-of-order RPC chunks.",
            "stdout_read" => "Ait could not read OMP stdout.",
            _ => diagnostic.category.message(),
        };
        Some(Cow::Owned(format!("Oh My Pi: {reason}")))
    }

    fn provider(&self) -> &'static str {
        "omp"
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async {
            // The manager asks immediately after submission, before polling any dialogs.
            // A queued get_state can wait on that same dialog and prevent its publication.
            if self.active.is_some() {
                let mut info = config::runtime(&self.state)?;
                info.mode_id.clone_from(&self.config.mode_id);
                return Ok(info);
            }
            let state = self.transport.request("get_state", json!({})).await?;
            if state["sessionId"] != self.state["sessionId"] {
                let id = config::text(&state, "sessionId")?;
                Uuid::parse_str(id).map_err(|_| AgentSessionError::Failed)?;
                let events = std::mem::take(&mut self.stream.events);
                self.stream = Stream::new(id.to_owned(), self.client.images.clone())
                    .with_blobs(history::root(&self.client)?.with_file_name("blobs"));
                self.stream.events = events;
                if let Some(turn) = &self.active {
                    self.stream.begin(turn.clone());
                }
                self.children = super::subagents::Children::default()
                    .with_blobs(history::root(&self.client)?.with_file_name("blobs"));
                self.created_at = chrono::Utc::now().to_rfc3339();
                self.submitted = false;
            }
            self.state = state;
            self.stream.set_context(&self.state);
            let mut info = config::runtime(&self.state)?;
            info.mode_id.clone_from(&self.config.mode_id);
            Ok(info)
        })
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.children.list()
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let file = self.state["sessionFile"].as_str()?;
        Some(AgentPersistenceHandle {
            provider: "omp".to_owned(),
            session_id: self.state["sessionId"].as_str()?.to_owned(),
            native_handle: None,
            metadata: Some(BTreeMap::from([
                ("cwd".to_owned(), json!(self.cwd)),
                ("sessionFile".to_owned(), json!(file)),
                ("inputIds".to_owned(), json!(self.stream.input_ids)),
                (
                    "empty".to_owned(),
                    json!(!self.submitted && !std::path::Path::new(file).is_file()),
                ),
                ("createdAt".to_owned(), json!(self.created_at)),
            ])),
        })
    }
    fn start_turn<'a>(
        &'a mut self,
        text: &'a str,
        config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async move { self.start(&AgentPrompt::text(text), config).await })
    }
    fn start_input<'a>(
        &'a mut self,
        prompt: &'a AgentPrompt,
        config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(self.start(prompt, config))
    }
    fn steer_input<'a>(
        &'a mut self,
        turn: &'a str,
        prompt: &'a AgentPrompt,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            prompt.validate()?;
            if self.closed
                || self.active.as_deref() != Some(turn)
                || prompt.output_schema.is_some()
                || self.input_requests.len() >= 128
            {
                return Err(AgentSessionError::Rejected);
            }
            let blocks = prompt.blocks()?;
            let text = blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            let images: Vec<_> = blocks
                .into_iter()
                .filter(|block| block["type"] == "image")
                .collect();
            let id = self
                .transport
                .begin("steer", json!({"message":text,"images":images}))
                .await?;
            self.input_requests.insert(id, turn.to_owned());
            self.invalidate_state_check();
            self.stream
                .pending_inputs
                .push_back(prompt.client_message_id.clone());
            Ok(())
        })
    }
    fn steer_turn<'a>(&'a mut self, turn: &'a str, text: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move { self.steer_input(turn, &AgentPrompt::text(text)).await })
    }
    fn cancel_turn<'a>(&'a mut self, turn: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.active.as_deref() != Some(turn) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            if self.cancellation_started.is_some() {
                return Ok(());
            }
            if self.cancel_requests.len() >= 128 {
                return Err(AgentSessionError::Failed);
            }
            let id = self
                .transport
                .begin("abort_and_restore_queue", json!({}))
                .await?;
            self.cancel_requests.insert(id, turn.to_owned());
            self.cancellation_started = Some(Instant::now());
            self.next_state_check = Instant::now();
            Ok(())
        })
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.dialogs.pending()
    }
    fn respond_permission<'a>(
        &'a mut self,
        id: &'a str,
        response: &'a Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            let native = self.dialogs.response(id, response)?;
            self.transport.send(&native).await?;
            self.dialogs.remove(id);
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
            Ok(())
        })
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        self.check_cancellation()?;
        if let Some(event) = self.stream.events.pop_front() {
            return Ok(Some(event));
        }
        for _ in 0..64 {
            let Some(event) = self.transport.poll()? else {
                self.poll_state_check()?;
                return Ok(None);
            };
            self.consume(&event).inspect_err(|_| {
                if self.transport.diagnostics.failure().is_none() {
                    let message = match event["type"].as_str() {
                        Some("extension_ui_request") => "Ait could not process an OMP question.",
                        Some("subagent_lifecycle" | "subagent_progress" | "subagent_event") => {
                            "Ait could not process an OMP subagent event."
                        }
                        _ => "Ait could not process an OMP session event.",
                    };
                    self.transport.diagnostics.observe_failure(
                        Diagnostic::new(Category::Protocol, "event"),
                        message,
                        "consume",
                    );
                }
            })?;
            if let Some(event) = self.stream.events.pop_front() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            self.closed = true;
            self.stream.events.extend(self.dialogs.clear());
            self.transport.close().await
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
