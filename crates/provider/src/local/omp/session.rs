use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

use super::{
    LABEL, OmpClient, PROVIDER, config, history, interactions::Dialogs, stream::Stream,
    transport::Transport,
};
use crate::local::prompt_blocks;
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
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

/// One native HTML render owned by this session; never a conversation writer.
/// A settled (expired/cancelled) record is retained until the late native response or
/// process close so a file created after the deadline is still cleaned exactly once.
#[derive(Debug)]
struct NativeExport {
    id: String,
    output: std::path::PathBuf,
    directory: std::path::PathBuf,
    file_name: String,
    started: Instant,
    abandoned: bool,
}

#[derive(Debug)]
pub(super) struct Session {
    client: OmpClient,
    transport: Transport,
    cwd: String,
    confinement: Option<domain::workspace::sandbox::WriteConfinement>,
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
    native_export: Option<NativeExport>,
    late_native_exports: Vec<NativeExport>,
}

/// Start a disposable native capability reader for `model` in `cwd`.
/// Returns invalid configuration, startup failures, or a mismatched native model selection.
pub(super) async fn probe(
    client: &OmpClient,
    cwd: &str,
    model: Option<&str>,
    confinement: Option<&domain::workspace::sandbox::WriteConfinement>,
) -> Result<Transport, AgentSessionError> {
    config::validate_spec(&AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig::default(),
        // Validation only; the borrowed policy is passed unchanged to spawn below.
        confinement: None,
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
    let mut transport = Transport::spawn(client, cwd, &args, confinement)?;
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
    let mut transport = Transport::spawn(client, &spec.cwd, &args, spec.confinement.as_ref())?;
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
        .service_tiers(
            &spec.cwd,
            runtime.model.as_deref(),
            spec.confinement.as_ref(),
        )
        .await?;
    Ok((transport, state, tiers))
}

pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    let resolved = match handle {
        Some(handle) => Some(super::aliases::resolve(client, handle, &spec.cwd).await?),
        None => None,
    };
    let handle = resolved.as_deref();
    let (journal, replayed) = match handle {
        Some(handle) => {
            let (journal, replayed) = history::resume(client, handle, &spec.cwd).await?;
            (Some(journal), replayed)
        }
        None => (None, None),
    };
    let path = journal
        .as_ref()
        .map(|journal| journal.path.to_str().ok_or(AgentSessionError::Failed))
        .transpose()?;
    let persisted = replayed.is_some();
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
    let mut stream = replayed.unwrap_or_else(|| Stream::new(id.to_owned(), client.images.clone()));
    stream = stream.with_blobs(history::root(client)?.with_file_name("blobs"));
    stream.set_context(&state);
    Ok(Session {
        client: client.clone(),
        transport,
        cwd: spec.cwd.clone(),
        confinement: spec.confinement.clone(),
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
        native_export: None,
        late_native_exports: Vec::new(),
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
                .service_tiers(
                    &self.cwd,
                    runtime.model.as_deref(),
                    self.confinement.as_ref(),
                )
                .await?;
            self.stream.set_context(&self.state);
            self.config.clone_from(config);
        } else {
            self.transport.close().await?;
            let path = config::text(&self.state, "sessionFile")?.to_owned();
            let spec = AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
                confinement: self.confinement.clone(),
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
            if let Some(export) = self.native_export.take() {
                remove_owned(&export.output);
            }
            for export in self.late_native_exports.drain(..) {
                remove_owned(&export.output);
            }
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

    /// Render the persisted native session with OMP's own HTML formatter. The render reads
    /// the journal and live state only: no prompt, resume, lease or second native process.
    /// The caller exclusively created `output` in the adapter's private export directory;
    /// this session owns that exact path and removes it on every terminal outcome.
    async fn begin_native_export(
        &mut self,
        output: std::path::PathBuf,
        directory: std::path::PathBuf,
    ) -> Result<String, AgentSessionError> {
        // One in-flight render plus a bounded set of settled records awaiting late cleanup.
        // Refuse new exports once the late set is full rather than evicting a record (which
        // would reintroduce the late-file leak); every retained record keeps its cleanup.
        if self.closed || self.native_export.is_some() || self.late_native_exports.len() >= 64 {
            return Err(AgentSessionError::Rejected);
        }
        // The public renderer refuses in-memory sessions; require the persisted identity.
        let journal = config::text(&self.state, "sessionFile")?;
        if !std::path::Path::new(journal).is_file() {
            return Err(AgentSessionError::Rejected);
        }
        let file_name = output
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        let id = self
            .transport
            .begin("export_html", json!({"outputPath": output}))
            .await?;
        self.native_export = Some(NativeExport {
            id: id.clone(),
            output,
            directory,
            file_name,
            started: Instant::now(),
            abandoned: false,
        });
        Ok(id)
    }

    /// Withdraw an export the caller no longer wants. The record is retained so a late
    /// native response still removes the owned file; no replacement export is started.
    fn abandon_native_export(&mut self) {
        if let Some(export) = self.native_export.take() {
            remove_owned(&export.output);
            self.late_native_exports.push(NativeExport {
                abandoned: true,
                ..export
            });
        }
    }

    /// Settle one correlated native export response without disturbing turn correlation.
    /// Handles both the in-flight record and retained late records (post-timeout/cancel),
    /// so a file the renderer creates after the deadline is still cleaned exactly once.
    fn complete_native_export(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        use crate::ports::native_html_export::{ExportedNativeHtml, NativeHtmlExportError};
        if event["command"] != "export_html" {
            return Err(AgentSessionError::Failed);
        }
        let id = event["id"].as_str().unwrap_or_default();
        let export = if self
            .native_export
            .as_ref()
            .is_some_and(|export| export.id == id)
        {
            self.native_export.take().ok_or(AgentSessionError::Failed)?
        } else if let Some(index) = self
            .late_native_exports
            .iter()
            .position(|export| export.id == id)
        {
            // Already settled (expired or cancelled): clean the late file, deliver nothing.
            let export = self.late_native_exports.remove(index);
            remove_owned(&export.output);
            return Ok(());
        } else {
            return Err(AgentSessionError::Failed);
        };
        let outcome = if event["success"] != true {
            self.transport
                .diagnostics
                .observe_native(&event["error"], "rpc", "export_html");
            Err(NativeHtmlExportError::Rejected)
        } else if event["data"]["path"].as_str() != Some(export.output.to_str().unwrap_or_default())
        {
            // The renderer must confirm the exact owned path; anything else is not adopted.
            Err(NativeHtmlExportError::Rejected)
        } else {
            adopt_native_html(
                &export.output,
                &export.directory,
                &self.state,
                &export.file_name,
            )
        };
        // Success and failure alike: the owned named staging file never survives adoption.
        remove_owned(&export.output);
        self.stream
            .events
            .push_back(AgentTurnEvent::NativeHtmlExport(
                outcome.map(|artifact| ExportedNativeHtml { artifact }),
            ));
        Ok(())
    }

    /// Fail an export whose native response never arrived; the turn itself stays intact.
    /// The record is retained (not dropped) so a late native response or process close
    /// still removes a file created after the deadline. No main kill, no retry.
    fn expire_native_export(&mut self) {
        use crate::ports::native_html_export::NativeHtmlExportError;
        if let Some(export) = self.native_export.take() {
            remove_owned(&export.output);
            if !export.abandoned {
                self.stream
                    .events
                    .push_back(AgentTurnEvent::NativeHtmlExport(Err(
                        NativeHtmlExportError::Io,
                    )));
            }
            self.late_native_exports.push(export);
        }
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
        let (text, images) = prompt_blocks::split(prompt.blocks()?);
        self.transport.diagnostics.clear();
        let prompt_id = self
            .transport
            .begin("prompt", json!({"message":text,"images":images}))
            .await?;
        self.stream
            .pending_inputs
            .push_back((prompt_id.clone(), prompt.client_message_id.clone()));
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
            self.transport
                .diagnostics
                .observe_native(&event["error"], "rpc", "prompt");
            self.stream.failed = true;
            self.finish();
        } else if event["success"] == true {
            self.phase = TurnPhase::Running;
            if event["data"]["agentInvoked"] == false {
                self.finish();
            }
        } else {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    fn control_response(&mut self, event: &Value) -> Result<bool, AgentSessionError> {
        if self
            .native_export
            .as_ref()
            .is_some_and(|export| event["id"] == export.id)
            || self
                .late_native_exports
                .iter()
                .any(|export| event["id"] == export.id)
        {
            self.complete_native_export(event)?;
            return Ok(true);
        }
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
            if self.active.as_deref() != Some(&turn)
                || event["command"] != "steer"
                || event["success"] != true
            {
                self.stream
                    .pending_inputs
                    .retain(|(request, _)| request != id);
            }
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
        if event["type"] == "message_end" && event["message"]["role"] == "assistant" {
            self.submitted = true;
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
            self.stream.pending_inputs.clear();
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

    /// An export that never answers fails alone; the conversation transport stays usable.
    fn check_native_export(&mut self) {
        if self
            .native_export
            .as_ref()
            .is_some_and(|export| export.started.elapsed() >= self.client.deadline)
        {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Timeout, "native_export"),
                "OMP did not finish the native HTML export in time.",
                "export_html",
            );
            self.expire_native_export();
        }
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

#[async_trait::async_trait]
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
        Some(Cow::Owned(format!("{LABEL}: {reason}")))
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
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
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.children.list()
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let file = self.state["sessionFile"].as_str()?;
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
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
    async fn start_turn(
        &mut self,
        text: &str,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        self.start(&AgentPrompt::text(text), config).await
    }
    async fn start_input(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        self.start(prompt, config).await
    }
    async fn steer_input(
        &mut self,
        turn: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        prompt.validate()?;
        if self.closed
            || self.active.as_deref() != Some(turn)
            || prompt.output_schema.is_some()
            || self.input_requests.len() >= 128
        {
            return Err(AgentSessionError::Rejected);
        }
        let (text, images) = prompt_blocks::split(prompt.blocks()?);
        let id = self
            .transport
            .begin("steer", json!({"message":text,"images":images}))
            .await?;
        self.input_requests.insert(id.clone(), turn.to_owned());
        self.invalidate_state_check();
        self.stream
            .pending_inputs
            .push_back((id, prompt.client_message_id.clone()));
        Ok(())
    }
    async fn steer_turn(&mut self, turn: &str, text: &str) -> Result<(), AgentSessionError> {
        self.steer_input(turn, &AgentPrompt::text(text)).await
    }
    async fn cancel_turn(&mut self, turn: &str) -> Result<(), AgentSessionError> {
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
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.dialogs.pending()
    }
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        let native = self.dialogs.response(id, response)?;
        self.transport.send(&native).await?;
        self.dialogs.remove(id);
        self.stream
            .events
            .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
        Ok(())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        self.check_cancellation()?;
        self.check_native_export();
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
    async fn begin_native_html_export(
        &mut self,
        output_path: &std::path::Path,
        export_directory: &std::path::Path,
    ) -> Result<String, AgentSessionError> {
        self.begin_native_export(output_path.to_path_buf(), export_directory.to_path_buf())
            .await
    }
    fn cancel_native_html_export(&mut self) {
        self.abandon_native_export();
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.closed = true;
        self.stream.events.extend(self.dialogs.clear());
        // Stop and reap the native writer first: a still-running renderer could otherwise
        // recreate an owned staging file after we unlink it. Only then remove owned paths.
        let closed = self.transport.close().await;
        if let Some(export) = self.native_export.take() {
            remove_owned(&export.output);
        }
        for export in self.late_native_exports.drain(..) {
            remove_owned(&export.output);
        }
        closed
    }
}

/// Delete only a staging entry this session created inside its private export directory.
/// `remove_file` unlinks the entry itself and never follows a symlink, so an owned link is
/// removed without touching its target; pre-existing foreign paths are never touched.
fn remove_owned(output: &std::path::Path) {
    match std::fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            let _ = std::fs::remove_file(output);
        }
        _ => {}
    }
}

/// Adopt the native renderer's confirmed output into an owned anonymous artifact.
/// The bytes are preserved verbatim: native HTML may contain active scripts and upstream
/// viewer resources, so it is delivered as a download and never rendered or sanitized here.
///
/// Ownership is validated against the exact private export directory (not the journal's
/// directory), and the file is opened with `O_NOFOLLOW` and read from that retained handle,
/// so a symlink swap between validation and read cannot redirect adoption to a foreign file.
fn adopt_native_html(
    output: &std::path::Path,
    directory: &std::path::Path,
    state: &Value,
    file_name: &str,
) -> Result<
    model::download::GeneratedArtifact,
    crate::ports::native_html_export::NativeHtmlExportError,
> {
    use crate::ports::native_html_export::NativeHtmlExportError;
    use std::io::{Read, Seek, Write};
    // Confinement: the adopted file must live in the exact owned private export directory
    // (a sibling of the native sessions root) under the exact owned basename.
    let canonical_directory =
        std::fs::canonicalize(directory).map_err(|_| NativeHtmlExportError::Io)?;
    let parent = output.parent().ok_or(NativeHtmlExportError::Rejected)?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|_| NativeHtmlExportError::Io)?;
    if canonical_parent != canonical_directory
        || output.file_name().and_then(|name| name.to_str()) != Some(file_name)
    {
        return Err(NativeHtmlExportError::Rejected);
    }
    // Reject a symlinked final component before opening; O_NOFOLLOW then closes the race.
    let link_metadata = std::fs::symlink_metadata(output).map_err(|_| NativeHtmlExportError::Io)?;
    if !link_metadata.file_type().is_file() {
        return Err(NativeHtmlExportError::Rejected);
    }
    let mut source = open_owned_nofollow(output).map_err(|_| NativeHtmlExportError::Rejected)?;
    let metadata = source.metadata().map_err(|_| NativeHtmlExportError::Io)?;
    if !metadata.is_file() {
        return Err(NativeHtmlExportError::Rejected);
    }
    if metadata.len() > model::download::MAX_ARTIFACT_BYTES {
        return Err(NativeHtmlExportError::TooLarge);
    }
    let agent_id = state["sessionId"]
        .as_str()
        .ok_or(NativeHtmlExportError::Rejected)?
        .to_owned();
    let mut artifact = tempfile::tempfile().map_err(|_| NativeHtmlExportError::Io)?;
    let mut buffer = vec![0u8; 32 * 1024];
    let mut size = 0u64;
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|_| NativeHtmlExportError::Io)?;
        if read == 0 {
            break;
        }
        size = size.saturating_add(read as u64);
        if size > model::download::MAX_ARTIFACT_BYTES {
            return Err(NativeHtmlExportError::TooLarge);
        }
        artifact
            .write_all(&buffer[..read])
            .map_err(|_| NativeHtmlExportError::Io)?;
    }
    artifact.rewind().map_err(|_| NativeHtmlExportError::Io)?;
    Ok(model::download::GeneratedArtifact {
        agent_id,
        file_name: file_name.to_owned(),
        mime_type: "text/html; charset=utf-8".to_owned(),
        size,
        file: artifact,
    })
}

/// Open an owned staging file for reading without following a final symlink.
#[cfg(unix)]
fn open_owned_nofollow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_owned_nofollow(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    // Non-unix adapters are not shipped; refuse rather than follow a symlink.
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "native export adoption requires unix no-follow open",
    ))
}

#[cfg(all(test, unix))]
mod tests;
