use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

use super::{
    LABEL, OmpClient, PROVIDER, config, history, interactions::Dialogs, roles, stream::Stream,
    transport::Transport,
};
use crate::local::prompt_blocks;
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent, ConfigApplyOutcome,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::protocol::prompt::AgentPrompt;

mod observed_settings;

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
    // A native settings observation scheduled this read; its reply refreshes model/thinking.
    refresh_settings: bool,
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
    // Declared after the transport: the private `--config` overlay must outlive its reader.
    overlay: Option<tempfile::NamedTempFile>,
    // Discovered at open; absent when native metadata could not be read, which only hides roles.
    catalog: Option<std::sync::Arc<roles::Catalog>>,
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
    settings_refresh: observed_settings::Refresh,
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

/// One live native process with the private role overlay that process must keep reading.
struct Connection {
    transport: Transport,
    // Owned for the connection's lifetime; `--config` is a path, not a copied snapshot.
    overlay: Option<tempfile::NamedTempFile>,
    state: Value,
    tiers: config::ServiceTiers,
}

async fn connect(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    path: Option<&str>,
) -> Result<Connection, AgentSessionError> {
    let mut args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(path) = path {
        args.extend(["--resume".to_owned(), path.to_owned()]);
    }
    // Explicit role assignments ride a private AIT-owned overlay; inherited roles add no file
    // and the native layered settings apply unchanged.
    let overlay = roles::overlay(&spec.config).await?;
    if let Some(file) = &overlay {
        let path = file.path().to_str().ok_or(AgentSessionError::Failed)?;
        args.extend(["--config".to_owned(), path.to_owned()]);
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
    let mut progress = config::LiveApply::default();
    config::apply(
        &mut transport,
        &StoredAgentConfig::default(),
        &spec.config,
        &mut progress,
    )
    .await?;
    if !progress.rejected.is_empty() {
        return Err(AgentSessionError::Rejected);
    }
    let state = progress.state.ok_or(AgentSessionError::Failed)?;
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
    Ok(Connection {
        transport,
        overlay,
        state,
        tiers,
    })
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
    let Connection {
        transport,
        overlay,
        state,
        tiers: service_tiers,
    } = connect(client, spec, path.filter(|_| persisted)).await?;
    let id = config::text(&state, "sessionId")?;
    if persisted && handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    let mut stream = replayed.unwrap_or_else(|| Stream::new(id.to_owned(), client.images.clone()));
    stream = stream.with_blobs(history::root(client)?.with_file_name("blobs"));
    stream.set_context(&state);
    let catalog = client.role_catalog(&spec.cwd).await.ok();
    Ok(Session {
        client: client.clone(),
        transport,
        overlay,
        catalog,
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
        settings_refresh: observed_settings::Refresh::default(),
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
    /// Apply native settings while idle for a turn; any rejected field refuses the turn.
    /// Invalid settings or an active turn reject before any native mutation.
    pub(super) async fn configure(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        let outcome = self.apply_config(config).await?;
        if !outcome.rejected.is_empty() {
            return Err(AgentSessionError::Rejected);
        }
        Ok(())
    }

    /// Apply a proposed configuration and report the native-confirmed result.
    ///
    /// Model/thinking/feature fields apply individually: rejected fields keep their prior
    /// values and the confirmed remainder is retained. Mode/system-prompt changes and cleared
    /// model/thinking overrides reopen the persisted session atomically under the same identity
    /// and confinement (the native setters cannot restore inheritance); a failed reopen
    /// restores the previous configuration with one recovery reconnect.
    pub(super) async fn apply_config(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<ConfigApplyOutcome, AgentSessionError> {
        config::validate(config)?;
        if self.closed {
            return Err(AgentSessionError::Rejected);
        }
        if self.active.is_some() {
            return Err(AgentSessionError::Busy);
        }
        if self.config == *config {
            return Ok(ConfigApplyOutcome {
                config: self.config.clone(),
                rejected: Vec::new(),
            });
        }
        self.validate_roles(config).await?;
        // Clearing a host override restores native inheritance, which only a reopen applies:
        // the live setters have no "unset" command.
        let clears_override = (self.config.model.is_some() && config.model.is_none())
            || (self.config.thinking_option_id.is_some() && config.thinking_option_id.is_none());
        // Role assignments reach OMP only through the private `--config` overlay a new process
        // reads at startup, so any assignment change reopens instead of using a live setter.
        let overlay_changed = !roles::same_overlay(&self.config, config);
        if !clears_override
            && !overlay_changed
            && self.config.mode_id == config.mode_id
            && self.config.system_prompt == config.system_prompt
        {
            self.apply_live_fields(config).await
        } else {
            let config = self.reopen(config).await?;
            Ok(ConfigApplyOutcome {
                config,
                rejected: Vec::new(),
            })
        }
    }

    /// Reject new role assignments the live catalog cannot resolve.
    ///
    /// Bounded shape is already checked by `config::validate`. This second gate compares the
    /// proposed assignments with the real role and model pools; a stored value the catalog
    /// cannot resolve stays display-only, so re-submitting it (other than "inherit") is
    /// rejected until a native read resolves it again.
    async fn validate_roles(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        let proposed = roles::overrides(config);
        if proposed.is_empty() || proposed == roles::overrides(&self.config) {
            return Ok(());
        }
        match self.client.role_catalog(&self.cwd).await {
            Ok(catalog) => catalog.validate(config),
            // No catalog means no authority to accept a new assignment; inherit still works
            // because it produces no overlay entry.
            Err(error) => {
                tracing::warn!(error=?error,"OMP role catalog unavailable; new assignments rejected");
                Err(AgentSessionError::Rejected)
            }
        }
    }

    /// Apply model/thinking/feature fields to the live native process and commit confirmed facts.
    async fn apply_live_fields(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<ConfigApplyOutcome, AgentSessionError> {
        let previous = self.config.clone();
        let mut progress = config::LiveApply::default();
        if let Err(error) =
            config::apply(&mut self.transport, &previous, config, &mut progress).await
        {
            // The transport is unusable after a failed request. Commit exactly the facts the
            // native session acknowledged before the failure — captured as each ack arrived —
            // and queue them as typed observations so the manager persists them despite the
            // error. Unacknowledged fields keep their prior values; nothing is rolled back or
            // invented from the proposal.
            let acknowledged = config::acknowledged(&previous, &progress);
            if acknowledged != previous {
                self.config.clone_from(&acknowledged);
                self.publish_observed_selection(&previous);
                self.publish_acknowledged_features(&previous, &progress);
            }
            return Err(error);
        }
        let state = progress.state.take().ok_or(AgentSessionError::Failed)?;
        let confirmed = config::confirmed(&previous, config, &state, &progress)?;
        // The readback is the authoritative selection. Drop selection events buffered before
        // this acknowledgement so a stale pre-ack change cannot revert the confirmed value;
        // events arriving after the ack remain authoritative. Unrelated frames are preserved.
        self.transport.supersede_buffered(&[
            "config_update",
            "model_changed",
            "thinking_level_changed",
        ]);
        self.commit_live(state, confirmed).await;
        Ok(ConfigApplyOutcome {
            config: self.config.clone(),
            rejected: progress.rejected,
        })
    }

    /// Queue the confirmed model/thinking delta as typed observations for durable commit.
    /// `runtime` carries the freshest native facts; on a failed application the last good
    /// state is used, since only the acknowledged selection changed.
    fn publish_observed_selection(&mut self, previous: &StoredAgentConfig) {
        use crate::protocol::agent_config::NullableSetting;
        let setting = |value: &Option<String>| {
            value
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set)
        };
        let model = if self.config.model == previous.model {
            NullableSetting::Unchanged
        } else {
            setting(&self.config.model)
        };
        let thinking_option_id = if self.config.thinking_option_id == previous.thinking_option_id {
            NullableSetting::Unchanged
        } else {
            setting(&self.config.thinking_option_id)
        };
        if !model.is_unchanged() || !thinking_option_id.is_unchanged() {
            self.stream
                .events
                .push_back(AgentTurnEvent::ModelSelectionUpdated {
                    model,
                    thinking_option_id,
                });
        }
        if let Ok(mut info) = config::runtime(&self.state) {
            info.model.clone_from(&self.config.model);
            info.thinking_option_id
                .clone_from(&self.config.thinking_option_id);
            info.mode_id.clone_from(&self.config.mode_id);
            self.stream
                .events
                .push_back(AgentTurnEvent::RuntimeInfo(info));
        }
    }

    /// Queue acknowledged feature toggles as a typed observation for durable commit.
    fn publish_acknowledged_features(
        &mut self,
        previous: &StoredAgentConfig,
        progress: &config::LiveApply,
    ) {
        let changed: BTreeMap<String, Value> = progress
            .acked_features
            .iter()
            .filter(|(id, value)| {
                previous
                    .feature_values
                    .as_ref()
                    .and_then(|values| values.get(*id))
                    != Some(value)
            })
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        if !changed.is_empty() {
            self.stream
                .events
                .push_back(AgentTurnEvent::FeatureValuesUpdated(changed));
        }
    }
    /// Commit a confirmed live configuration and its native state snapshot.
    /// The optional service-tier probe runs afterwards; its failure degrades the advertised
    /// toggles but never discards confirmed native facts.
    async fn commit_live(&mut self, state: Value, config: StoredAgentConfig) {
        let runtime = config::runtime(&state).ok();
        self.state = state;
        self.stream.set_context(&self.state);
        self.config = config;
        self.service_tiers = self
            .client
            .service_tiers(
                &self.cwd,
                runtime.as_ref().and_then(|info| info.model.as_deref()),
                self.confinement.as_ref(),
            )
            .await
            .unwrap_or_default();
    }

    /// Reopen the persisted session for mode/system-prompt changes, atomically per attempt.
    /// A failed reopen restores the previous configuration with one recovery reconnect;
    /// no user input is replayed and the confinement is never relaxed.
    async fn reopen(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<StoredAgentConfig, AgentSessionError> {
        let previous = self.config.clone();
        let path = config::text(&self.state, "sessionFile")?.to_owned();
        let persisted = std::path::Path::new(&path).is_file();
        self.transport.close().await?;
        let spec = AgentSessionSpec {
            provider: PROVIDER.to_owned(),
            cwd: self.cwd.clone(),
            config: config.clone(),
            confinement: self.confinement.clone(),
        };
        match connect(&self.client, &spec, persisted.then_some(path.as_str())).await {
            Ok(connection) => {
                if persisted && connection.state["sessionId"] != self.state["sessionId"] {
                    return Err(AgentSessionError::Failed);
                }
                self.adopt_reopen(connection, persisted, config.clone())?;
                Ok(self.config.clone())
            }
            Err(error) => {
                let recovery = AgentSessionSpec {
                    config: previous.clone(),
                    ..spec
                };
                let connection =
                    connect(&self.client, &recovery, persisted.then_some(path.as_str()))
                        .await
                        .map_err(|_| error)?;
                self.adopt_reopen(connection, persisted, previous)?;
                Err(error)
            }
        }
    }

    /// Install a reopened native process, retaining the stream of a persisted session.
    fn adopt_reopen(
        &mut self,
        connection: Connection,
        persisted: bool,
        config: StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        let Connection {
            transport,
            overlay,
            state,
            tiers: service_tiers,
        } = connection;
        if !persisted {
            self.stream = Stream::new(
                config::text(&state, "sessionId")?.to_owned(),
                self.client.images.clone(),
            )
            .with_blobs(history::root(&self.client)?.with_file_name("blobs"));
        }
        self.transport = transport;
        self.overlay = overlay;
        // Same cwd/environment identity: the cached catalog still describes this profile.
        self.state_check = None;
        self.settings_refresh = observed_settings::Refresh::default();
        self.cancel_requests.clear();
        self.input_requests.clear();
        self.idle_without_outcome = None;
        self.state = state;
        self.service_tiers = service_tiers;
        self.stream.set_context(&self.state);
        self.config = config;
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
            Some("config_update") => self.observe_settings(event)?,
            Some("model_changed" | "thinking_level_changed") => self.settings_refresh.mark(),
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
        if check.refresh_settings && event["command"] == "get_state" && event["success"] == true {
            let state = &event["data"];
            if state["sessionId"] != self.state["sessionId"] {
                return Err(AgentSessionError::Failed);
            }
            self.observe_settings(state)?;
        }
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
        let turn = self.active.as_ref().filter(|_| {
            matches!(
                self.phase,
                TurnPhase::Running | TurnPhase::AwaitingSettlement
            )
        });
        if self.state_check.is_some()
            || (turn.is_none() && !self.settings_refresh.pending())
            || (!self.settings_refresh.pending() && Instant::now() < self.next_state_check)
        {
            return Ok(());
        }
        let (id, written) = match self.transport.check_state() {
            Ok(check) => check,
            Err(AgentSessionError::Busy) => return Ok(()),
            Err(error) => return Err(error),
        };
        self.state_check = Some(StateCheck {
            id,
            turn: turn.cloned(),
            refresh_settings: self.settings_refresh.take(),
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
    async fn apply_config(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<ConfigApplyOutcome, AgentSessionError> {
        Session::apply_config(self, config).await
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        let tiers = if config.model == self.config.model {
            self.service_tiers
        } else {
            config::ServiceTiers::default()
        };
        let mut features = config::features(config, tiers);
        features.extend(match &self.catalog {
            Some(catalog) => catalog.features(config),
            // Without a catalog only the persisted role keys render; no fixed set is invented.
            None => roles::stored_features(config),
        });
        Some(json!({"availableModes":config::modes(),"features":features}))
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
        self.observe_settings(&state)?;
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
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.closed = true;
        self.stream.events.extend(self.dialogs.clear());
        self.transport.close().await
    }
}

#[cfg(all(test, unix))]
mod tests;
