use crate::ports::agent_session::AgentSessionFuture;
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

mod observed_settings;
mod side_commands;
mod tasks;

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
    cwd: String,
    confinement: Option<domain::workspace::sandbox::WriteConfinement>,
    state: Value,
    config: StoredAgentConfig,
    service_tiers: config::ServiceTiers,
    role_catalog: std::sync::Arc<super::roles::Catalog>,
    stream: Stream,
    dialogs: Dialogs,
    ui: super::ui::State,
    actions: super::actions::Pending,
    navigation: Option<Value>,
    navigation_target: Option<String>,
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
    commands: Vec<Value>,
    native_commands: Vec<Value>,
    command_controls: super::commands::pending::Pending,
    btw: super::btw::State,
    vibe: super::vibe::State,
    vibe_host: Option<super::vibe::host::Host>,
    active_builtin: bool,
    command_version: u64,
}

/// Start a disposable native capability reader for `model` in `cwd`.
/// Returns invalid configuration, startup failures, or a mismatched native model selection.
pub(super) async fn probe(
    client: &OmpClient,
    cwd: &str,
    model: Option<&str>,
) -> Result<Transport, AgentSessionError> {
    probe_config(client, cwd, model, None).await
}

/// Read capabilities with the proposed native role overlay and no persistent session.
/// Returns startup, invalid configuration or mismatched explicit model errors.
pub(super) async fn probe_config(
    client: &OmpClient,
    cwd: &str,
    model: Option<&str>,
    config: Option<&StoredAgentConfig>,
) -> Result<Transport, AgentSessionError> {
    probe_options(
        client,
        cwd,
        Probe {
            model,
            config,
            ..Probe::default()
        },
    )
    .await
}

#[derive(Default)]
struct Probe<'a> {
    model: Option<&'a str>,
    config: Option<&'a StoredAgentConfig>,
    extension: Option<&'a std::path::Path>,
    interactive: bool,
}

/// Discover commands without asking a user to answer a disposable metadata session's dialogs.
/// Native project extensions still load; their documented headless fallback remains in control.
pub(super) async fn probe_commands(
    client: &OmpClient,
    cwd: &str,
    extension: Option<&std::path::Path>,
) -> Result<Transport, AgentSessionError> {
    probe_options(
        client,
        cwd,
        Probe {
            extension,
            ..Probe::default()
        },
    )
    .await
}

/// Load one temporary public extension in an isolated metadata reader.
/// The caller retains `extension` until initialization and close have completed.
pub(super) async fn probe_config_with_extension(
    client: &OmpClient,
    cwd: &str,
    model: Option<&str>,
    config: Option<&StoredAgentConfig>,
    extension: Option<&std::path::Path>,
) -> Result<Transport, AgentSessionError> {
    probe_options(
        client,
        cwd,
        Probe {
            model,
            config,
            extension,
            interactive: true,
        },
    )
    .await
}

async fn probe_options(
    client: &OmpClient,
    cwd: &str,
    options: Probe<'_>,
) -> Result<Transport, AgentSessionError> {
    let Probe {
        model,
        config,
        extension,
        interactive,
    } = options;
    config::validate_spec(&AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    })?;
    let mut args = vec![
        "--mode".to_owned(),
        if interactive { "rpc-ui" } else { "rpc" }.to_owned(),
        "--no-session".to_owned(),
    ];
    if !interactive {
        args.push("--no-ui".to_owned());
    }
    let route = model.map(config::model_route).transpose()?;
    if let Some(extension) = extension {
        args.push("--extension".to_owned());
        args.push(
            extension
                .to_str()
                .ok_or(AgentSessionError::Failed)?
                .to_owned(),
        );
    }
    let configuration = if let Some(config) = config {
        super::roles::overlay(config).await?
    } else {
        None
    };
    if let Some(configuration) = &configuration {
        args.extend([
            "--config".to_owned(),
            configuration
                .path()
                .to_str()
                .ok_or(AgentSessionError::Failed)?
                .to_owned(),
        ]);
    }
    if let Some([provider, model]) = &route {
        args.extend([
            "--provider".to_owned(),
            provider.clone(),
            "--model".to_owned(),
            model.clone(),
        ]);
    }
    let mut transport = Transport::spawn(client, cwd, &args, None)?;
    transport.retain_configuration(configuration);
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
    roles: &super::roles::Catalog,
    capabilities: (Option<String>, config::ServiceTiers),
) -> Result<(Transport, Value, config::ServiceTiers), AgentSessionError> {
    roles.validate(&spec.config)?;
    let configuration = super::roles::overlay(&spec.config).await?;
    let mut args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    let extension = client
        .vibe_host
        .then(super::vibe::host::Host::extension)
        .transpose()?;
    if let Some(extension) = &extension {
        args.extend([
            "--extension".to_owned(),
            extension.path().to_string_lossy().into_owned(),
        ]);
    }
    if let Some(configuration) = &configuration {
        args.extend([
            "--config".to_owned(),
            configuration
                .path()
                .to_str()
                .ok_or(AgentSessionError::Failed)?
                .to_owned(),
        ]);
    }
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
    let startup = client
        .startup
        .lock()
        .map_err(|_| AgentSessionError::Failed)?
        .clone();
    if let Some(startup) = startup {
        transport.startup(startup).await?;
    }

    transport.retain_configuration(configuration);
    transport.retain_extension(extension);
    transport.initialize().await?;
    let state = config::apply(&mut transport, &spec.config).await?;
    transport
        .request("set_ask_dialog", json!({"enabled":true}))
        .await?;
    transport
        .request("set_subagent_subscription", json!({"level":"events"}))
        .await?;
    let runtime = config::runtime(&state)?;
    let tiers = if runtime.model == capabilities.0 {
        capabilities.1
    } else {
        client
            .service_tiers(&spec.cwd, runtime.model.as_deref())
            .await?
    };
    Ok((transport, state, tiers))
}

/// Session prelude recovered before the native process starts: the journal to replay,
/// the path to resume from, and the creation timestamp the transcript reports.
struct Restored {
    journal: Option<history::Journal>,
    replayed: Option<Stream>,
    resume: Option<String>,
    created_at: String,
}

async fn restore(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Restored, AgentSessionError> {
    let (journal, replayed) = match handle {
        Some(handle) => {
            let (journal, replayed) = history::resume(client, handle, &spec.cwd).await?;
            (Some(journal), replayed)
        }
        None => (None, None),
    };
    let resume = journal
        .as_ref()
        .map(|journal| journal.path.to_str().ok_or(AgentSessionError::Failed))
        .transpose()?
        .map(str::to_owned);
    let created_at = journal
        .as_ref()
        .and_then(|journal| journal.header["timestamp"].as_str())
        .map_or_else(|| chrono::Utc::now().to_rfc3339(), str::to_owned);
    Ok(Restored {
        journal,
        replayed,
        resume,
        created_at,
    })
}

pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    open_scoped(client, spec, handle, None).await
}

/// Open or resume an external worker under its verified parent's native journal.
pub(super) async fn open_worker(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    parent: &str,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    if let Some(handle) = handle {
        let journal = history::load(client, handle, &spec.cwd).await?;
        if journal.header["parentSession"] != parent {
            return Err(AgentSessionError::Rejected);
        }
    }
    let session = open_scoped(client, spec, handle, Some(parent)).await?;
    let identity = session.persistence().ok_or(AgentSessionError::Failed)?;
    let journal = history::load(client, &identity, &spec.cwd).await?;
    if journal.header["parentSession"] != parent {
        return Err(AgentSessionError::Rejected);
    }
    Ok(session)
}

async fn open_scoped(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
    parent: Option<&str>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    let Restored {
        mut journal,
        replayed,
        resume,
        created_at,
    } = restore(client, spec, handle).await?;
    let persisted = replayed.is_some();
    let (role_catalog, tiers) = client.role_settings(spec).await?;
    let (mut transport, mut state, service_tiers) = connect(
        client,
        spec,
        resume.as_deref().filter(|_| persisted),
        &role_catalog,
        tiers,
    )
    .await?;
    if let Some(parent) = parent.filter(|_| handle.is_none()) {
        state = new_worker(&mut transport, &spec.config, parent).await?;
    }
    let commands = config::commands(
        &transport
            .request("get_available_commands", json!({}))
            .await?,
    )?;
    let id = config::text(&state, "sessionId")?;
    if persisted && handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    restore_navigation(client, &mut transport, handle, &mut journal).await?;
    let navigation = journal
        .as_ref()
        .and_then(|journal| journal.navigation.clone());
    let mut stream = if navigation.is_some() && persisted {
        let client = client.clone();
        let handle = handle.ok_or(AgentSessionError::Failed)?.clone();
        tokio::task::spawn_blocking(move || {
            journal
                .ok_or(AgentSessionError::Failed)?
                .project(&client, &handle)
                .map(|(_, stream)| stream)
        })
        .await
        .map_err(|_| AgentSessionError::Failed)??
    } else {
        replayed.unwrap_or_else(|| Stream::new(id.to_owned(), client.images.clone()))
    };
    stream = stream.with_blobs(history::root(client)?.with_file_name("blobs"));
    stream.set_context(&state);
    let (btw, vibe) = native_controls(&mut transport).await;
    let vibe_host = if vibe.available {
        None
    } else {
        super::vibe::host::Host::discover(client, spec, &state, &commands, handle)?
    };
    let visible_commands = super::vibe::host::visible_commands(&commands, vibe_host.is_some());
    Ok(Session {
        client: client.clone(),
        transport,
        cwd: spec.cwd.clone(),
        confinement: spec.confinement.clone(),
        state,
        config: spec.config.clone(),
        service_tiers,
        role_catalog,
        stream,
        dialogs: Dialogs::default(),
        ui: super::ui::State::default(),
        actions: super::actions::Pending::default(),
        navigation,
        navigation_target: None,
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
        commands: visible_commands,
        native_commands: commands,
        command_controls: super::commands::pending::Pending::default(),
        btw,
        vibe,
        vibe_host,
        active_builtin: false,
        command_version: 1,
    })
}

async fn new_worker(
    transport: &mut Transport,
    config: &StoredAgentConfig,
    parent: &str,
) -> Result<Value, AgentSessionError> {
    let changed = transport
        .request("new_session", json!({"parentSession":parent}))
        .await?;
    if changed["cancelled"] == true {
        return Err(AgentSessionError::Rejected);
    }
    config::apply(transport, config).await
}

async fn native_controls(transport: &mut Transport) -> (super::btw::State, super::vibe::State) {
    let mut btw = super::btw::State::default();
    if let Ok(history) = transport.request("get_btw_history", json!({})).await {
        btw.restore(&history).unwrap_or_else(|_| btw.malformed());
    }
    let vibe = super::vibe::State {
        available: transport
            .refresh_capability()
            .is_some_and(|capability| capability.vibe),
        ..super::vibe::State::default()
    };
    (btw, vibe)
}

async fn restore_navigation(
    client: &OmpClient,
    transport: &mut Transport,
    handle: Option<&AgentPersistenceHandle>,
    journal: &mut Option<history::Journal>,
) -> Result<(), AgentSessionError> {
    let Some(navigation) = journal
        .as_ref()
        .and_then(|journal| journal.navigation.clone())
    else {
        return Ok(());
    };
    let target = config::text(&navigation, "targetId")?;
    let result = transport
        .request("navigate_tree", json!({"entryId":target}))
        .await?;
    if result["cancelled"] == true || result["aborted"] == true {
        return Err(AgentSessionError::Rejected);
    }
    let native = transport.request("get_entries", json!({})).await?;
    if native.get("leafId") != navigation.get("leafId") {
        return Err(AgentSessionError::Failed);
    }
    let cwd = journal.as_ref().ok_or(AgentSessionError::Failed)?.header["cwd"]
        .as_str()
        .ok_or(AgentSessionError::Failed)?;
    let mut restored = history::load(client, handle.ok_or(AgentSessionError::Failed)?, cwd).await?;
    restored.set_navigation(target, &navigation["leafId"])?;
    *journal = Some(restored);
    Ok(())
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
    /// Revision of the most recently observed native command catalog, for ACP notifications.
    pub(super) fn command_version(&self) -> u64 {
        self.command_version
    }

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
        if self
            .vibe_host
            .as_ref()
            .is_some_and(super::vibe::host::Host::running)
        {
            return Err(AgentSessionError::Busy);
        }
        let enabled = self
            .vibe_host
            .as_ref()
            .is_some_and(super::vibe::host::Host::enabled);
        let workers = self
            .vibe_host
            .as_ref()
            .map(super::vibe::host::Host::persisted);
        if let Some(host) = &mut self.vibe_host {
            host.toggle(&mut self.transport, false).await?;
        }
        self.apply_connection_config(config).await?;
        if self.vibe_host.is_some() {
            let spec = AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
                confinement: self.confinement.clone(),
            };
            let mut host = super::vibe::host::Host::new(
                &self.client,
                &spec,
                config::text(&self.state, "sessionId")?,
                config::text(&self.state, "sessionFile")?,
                workers.as_ref(),
            );
            if enabled {
                host.toggle(&mut self.transport, true).await?;
            }
            self.vibe_host = Some(host);
        }
        Ok(())
    }

    async fn apply_connection_config(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        if self.config.mode_id == config.mode_id
            && self.config.system_prompt == config.system_prompt
            && super::roles::same_overlay(&self.config, config)
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
            if self.btw.running() || self.command_controls.status().is_some() {
                return Err(AgentSessionError::Busy);
            }
            if !super::roles::same_overlay(&self.config, config) {
                let state = self.transport.request("get_state", json!({})).await?;
                if state["hasPendingAsyncWork"] != false
                    || state["isStreaming"] != false
                    || state["isCompacting"] != false
                    || state["isSettled"] != true
                    || state["queuedMessageCount"].as_u64() != Some(0)
                {
                    return Err(AgentSessionError::Busy);
                }
            }
            let path = config::text(&self.state, "sessionFile")?.to_owned();
            let spec = AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
                confinement: self.confinement.clone(),
            };
            let (role_catalog, tiers) = self.client.role_settings(&spec).await?;
            self.transport.close().await?;
            let persisted = std::path::Path::new(&path).is_file();
            let (mut transport, state, service_tiers) = connect(
                &self.client,
                &spec,
                persisted.then_some(path.as_str()),
                &role_catalog,
                tiers,
            )
            .await?;
            let commands = config::commands(
                &transport
                    .request("get_available_commands", json!({}))
                    .await?,
            )?;
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
            self.command_controls.close();
            self.btw = super::btw::State::default();
            self.vibe = super::vibe::State {
                available: self
                    .transport
                    .refresh_capability()
                    .is_some_and(|capability| capability.vibe),
                ..super::vibe::State::default()
            };
            if let Ok(history) = self.transport.request("get_btw_history", json!({})).await {
                self.btw
                    .restore(&history)
                    .unwrap_or_else(|_| self.btw.malformed());
            }
            self.state_check = None;
            self.cancel_requests.clear();
            self.input_requests.clear();
            self.idle_without_outcome = None;
            self.state = state;
            self.service_tiers = service_tiers;
            self.role_catalog = role_catalog;
            self.stream.set_context(&self.state);
            self.config.clone_from(config);
            self.update_commands(commands);
        }
        Ok(())
    }

    /// Hand startup callbacks to the established native owner without replaying plugin input.
    pub(super) fn adopt_startup(&mut self, dialogs: Dialogs, ui: super::ui::State) {
        self.dialogs = dialogs;
        self.ui = ui;
        self.transport.finish_startup();
    }

    fn update_commands(&mut self, commands: Vec<Value>) {
        if self.native_commands != commands {
            self.commands =
                super::vibe::host::visible_commands(&commands, self.vibe_host.is_some());
            self.native_commands = commands;
            self.command_version = self.command_version.wrapping_add(1);
        }
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
        if enabled {
            self.vibe_host.take();
            self.commands = super::vibe::host::visible_commands(&self.native_commands, false);
        }
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
        if self.closed
            || self.active.is_some()
            || self.actions.changing_history()
            || prompt.output_schema.is_some()
        {
            return Err(AgentSessionError::Rejected);
        }
        self.configure(config).await?;
        let mut task_directive = self.prepare_goal(prompt).await?;
        if let Some(turn) = task_directive.native_turn.take() {
            return Ok(turn);
        }
        let prompt = task_directive.prompt.as_ref().unwrap_or(prompt);
        let mut directive = None;
        if super::commands::owns_control(&prompt.text, &self.native_commands, "vibe")
            && self.vibe_host.is_some()
        {
            let (_, args) =
                super::commands::split(&prompt.text).ok_or(AgentSessionError::Rejected)?;
            if args.is_empty() {
                return Err(AgentSessionError::Rejected);
            }
            self.vibe_host
                .as_mut()
                .ok_or(AgentSessionError::Unavailable)?
                .toggle(&mut self.transport, true)
                .await?;
            let mut input = prompt.clone();
            args.clone_into(&mut input.text);
            directive = Some(input);
        }
        let prompt = directive.as_ref().unwrap_or(prompt);
        if let Some(name) =
            super::commands::blocked(&prompt.text, &self.native_commands, self.vibe.available)
        {
            let turn = Uuid::new_v4().to_string();
            self.reject_command(prompt, name);
            self.stream
                .events
                .push_back(AgentTurnEvent::Completed(None));
            return Ok(turn);
        }
        if super::commands::owns_control(&prompt.text, &self.native_commands, "btw") {
            return Err(AgentSessionError::Rejected);
        }
        self.active_builtin = super::commands::builtin(&prompt.text, &self.native_commands);
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
        if event["type"] == "agent_start" {
            self.invalidate_state_check();
        }

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
                            | "open_url"
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

    /// Project adapter Vibe frames captured by the reader into host state and child events.
    /// The native session id is required: a descendant's parent must be the root or a known worker.
    fn drain_vibe(&mut self) -> Result<(), AgentSessionError> {
        let frames = self.transport.take_vibe_frames();
        if frames.is_empty() {
            return Ok(());
        }
        let root = self.state["sessionId"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
            .to_owned();
        for frame in &frames {
            if let Some(event) = self.vibe.capture(frame, &root, &self.cwd) {
                self.stream.events.push_back(event);
            }
        }
        Ok(())
    }

    fn consume_side_control(&mut self, event: &Value) -> bool {
        if self.btw.capture(event).is_err() {
            self.btw.malformed();
            tracing::warn!("OMP side-question payload was rejected");
        }
        match self.command_controls.capture(event) {
            super::commands::pending::Capture::Unrelated => false,
            super::commands::pending::Capture::Pending => true,
            super::commands::pending::Capture::Finished(entry) => {
                self.stream
                    .events
                    .push_back(AgentTurnEvent::Timeline(entry));
                true
            }
        }
    }

    fn consume_vibe_tool(&mut self, event: &Value) -> Result<bool, AgentSessionError> {
        if matches!(
            event["type"].as_str(),
            Some("host_tool_call" | "host_tool_cancel")
        ) {
            let writer = self.transport.writer()?;
            if let Some(host) = &mut self.vibe_host
                && host.tool(event, writer)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if self.consume_vibe_tool(event)? {
            return Ok(());
        }
        if self.consume_side_control(event) {
            return Ok(());
        }
        if event["type"] == "response" && self.actions.capture(event) {
            self.publish_action_response(event)?;
            return Ok(());
        }
        if event["type"] == "response" && self.control_response(event)? {
            return Ok(());
        }
        self.observe_native_event(event)?;
        self.observe_assistant_result(event);
        match event["type"].as_str() {
            Some("config_update") => self.observe_settings(event)?,
            Some("model_changed" | "thinking_level_changed") => self.settings_refresh.mark(),
            Some("goal_updated") => self.observe_goal(&event["state"]),
            Some("available_commands_update") => {
                self.update_commands(config::commands(event)?);
            }
            Some("extension_ui_request") => {
                self.ui_request(event)?;
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

    fn observe_assistant_result(&mut self, event: &Value) {
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
    }

    fn publish_action_response(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        for values in self.actions.feature_updates() {
            self.config
                .feature_values
                .get_or_insert_default()
                .extend(values.clone());
            self.stream
                .events
                .push_back(AgentTurnEvent::FeatureValuesUpdated(values));
        }
        self.stream
            .events
            .extend(self.actions.notes().map(AgentTurnEvent::Timeline));
        self.ui.capture(event)?;
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

    fn ui_request(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        self.ui.capture(event)?;
        let events = self.dialogs.capture(event)?;
        if events.is_empty() {
            self.project(event)?;
        }
        self.stream.events.extend(events);
        Ok(())
    }

    fn finish(&mut self) {
        if self.active.take().is_some() {
            self.active_builtin = false;
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
    fn handles_out_of_band(&self, text: &str) -> Option<bool> {
        if self.owns_goal(text) {
            return Some(!tasks::starts_goal(text));
        }
        // A `/vibe` the bridge cannot serve is answered here, as a refusal card that never
        // reaches a model. Once the native host announces the capability, `/vibe` is a normal
        // prompt directive and must form a turn instead.
        if self.vibe_host.is_some()
            && super::commands::owns_control(text, &self.native_commands, "vibe")
        {
            return Some(super::commands::split(text).is_some_and(|(_, args)| args.is_empty()));
        }
        Some(
            super::commands::owns_control(text, &self.native_commands, "btw")
                || (super::commands::owns_control(text, &self.native_commands, "vibe")
                    && !self.vibe.available)
                || super::commands::blocked(text, &self.native_commands, self.vibe.available)
                    .is_some()
                || super::commands::read_only(text, &self.native_commands),
        )
    }
    async fn out_of_band(&mut self, prompt: &AgentPrompt) -> Result<(), AgentSessionError> {
        self.side_command(prompt).await
    }
    fn take_command_completion(&mut self) -> Option<AgentSessionFuture<'static, ()>> {
        self.command_controls.take()
    }
    async fn complete_commands(
        &mut self,
        input: &crate::protocol::commands::CompletionInput,
    ) -> Result<crate::protocol::commands::ValidatedCompletion, AgentSessionError> {
        super::completion::complete(
            &mut self.transport,
            input,
            &self.native_commands,
            self.vibe.available,
        )
        .await
    }
    fn commands(&self) -> Option<&[Value]> {
        Some(&self.commands)
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        if let Some(catalog) = self.client.cached_roles(&self.cwd)? {
            catalog.validate(config)?;
        }
        // A cache miss must reach the client's current-directory validation, never an old copy.
        Ok(())
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        let tiers = if config.model == self.config.model {
            self.service_tiers
        } else {
            config::ServiceTiers::default()
        };
        let mut features = config::features(config, tiers);
        let catalog = match self.client.cached_roles(&self.cwd) {
            Ok(catalog) => catalog,
            Err(error) => {
                tracing::warn!(?error, "OMP role catalog could not be read");
                return None;
            }
        };
        features.extend(
            catalog
                .as_ref()
                .unwrap_or(&self.role_catalog)
                .features(config),
        );
        Some(json!({"availableModes":config::modes(),"features":features}))
    }

    fn ui_state(&self) -> Option<Value> {
        if self.closed {
            return None;
        }
        let mut snapshot = self.ui.snapshot();
        snapshot["actions"] = super::actions::catalog();
        snapshot["sideQuestions"] = self.btw.snapshot();
        snapshot["goal"] = if matches!(
            self.state["goal"]["goal"]["status"].as_str(),
            Some("active" | "paused" | "blocked")
        ) {
            tasks::goal_item(
                "current-goal",
                &json!({"goal": self.state["goal"]["goal"], "state":self.state["goal"]}),
            )
            .item
        } else {
            Value::Null
        };
        if let Some(host) = &self.vibe_host {
            snapshot["vibe"] = host.snapshot();
        } else if self.vibe.available {
            snapshot["vibe"] = self.vibe.snapshot();
        }
        if let Some(status) = self.command_controls.status()
            && let Some(statuses) = snapshot["statuses"]
                .as_array_mut()
                .filter(|statuses| statuses.len() < 32)
        {
            statuses.push(json!({"key":"ait-native-query","text":status}));
        }
        Some(snapshot)
    }
    fn observation_committed(&mut self) {
        if self.stream.events.is_empty() {
            self.actions.committed();
        }
    }

    async fn begin_action(
        &mut self,
        action: &str,
        arguments: &Value,
    ) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Unavailable);
        }
        if matches!(action, "model_preset_save" | "model_preset_delete") {
            return super::roles::manage_preset(
                self.client.clone(),
                self.cwd.clone(),
                self.config.clone(),
                action,
                arguments,
            );
        }
        if (self.actions.changing_history() && action != "abort_branch_summary")
            || (super::actions::changes_history(action) && self.active.is_some())
        {
            return Err(AgentSessionError::Busy);
        }
        if super::actions::changes_history(action)
            && self
                .vibe_host
                .as_ref()
                .is_some_and(super::vibe::host::Host::enabled)
        {
            return Err(AgentSessionError::Busy);
        }
        if action == "goal" && arguments.get("expectedGoalId").is_some() {
            return self.current_goal_action(arguments).await;
        }
        let present = arguments.get("present").is_some();
        let mut stripped;
        let arguments = if present {
            if arguments["present"] != "true"
                || !matches!(
                    action,
                    "get_session_stats" | "get_login_providers" | "get_logout_accounts"
                )
            {
                return Err(AgentSessionError::Rejected);
            }
            stripped = arguments.clone();
            stripped
                .as_object_mut()
                .ok_or(AgentSessionError::Rejected)?
                .remove("present");
            &stripped
        } else {
            arguments
        };
        let fields = super::actions::validate(action, arguments, &self.cwd)?;
        if action == "vibe_control" && self.vibe_host.is_some() {
            if fields["operation"] == "toggle" {
                let host = self
                    .vibe_host
                    .as_mut()
                    .ok_or(AgentSessionError::Unavailable)?;
                let enabled = fields["enabled"].as_bool().unwrap_or(!host.enabled());
                if enabled && self.active.is_some() {
                    return Err(AgentSessionError::Busy);
                }
                host.toggle(&mut self.transport, enabled).await?;
                let snapshot = host.snapshot();
                return Ok(Box::pin(async move {
                    Ok(json!({"text":snapshot.to_string(),"error":null}))
                }));
            }
            return Ok(self
                .vibe_host
                .as_ref()
                .ok_or(AgentSessionError::Unavailable)?
                .control(&fields));
        }

        if action == "vibe_control" {
            // Native Vibe controls exist only behind the bridge handshake; without it the
            // catalog entry stays inert instead of reaching a runtime that cannot serve it.
            if !self.vibe.available {
                return Err(AgentSessionError::Unavailable);
            }
            let id = self.transport.begin("ait_vibe_control", fields).await?;
            return Ok(self.actions.insert(id, "ait_vibe_control"));
        }
        if matches!(action, "cancel_subagent" | "steer_subagent") {
            let owned = self.transport.request("get_subagents", json!({})).await?;
            super::actions::validate_child(&owned, &fields["subagentId"])?;
        }
        self.actions.check_capacity()?;
        let target = fields["entryId"].as_str().map(str::to_owned);
        let feature =
            super::actions::runtime_feature(action).map(|key| (key, fields["enabled"].clone()));
        let id = self.transport.begin(action, fields).await?;
        if let Some((key, value)) = feature {
            return Ok(self.actions.insert_feature(id, action, key, value));
        }
        if action == "navigate_tree" {
            self.navigation_target = target;
        }
        Ok(self.actions.insert_with_presentation(id, action, present))
    }

    async fn begin_configured_action(
        &mut self,
        action: &str,
        arguments: &Value,
        config: &StoredAgentConfig,
    ) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Unavailable);
        }
        if matches!(action, "model_preset_save" | "model_preset_delete") {
            return super::roles::manage_preset(
                self.client.clone(),
                self.cwd.clone(),
                config.clone(),
                action,
                arguments,
            );
        }
        self.begin_action(action, arguments).await
    }

    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.transport.diagnostics.failure()
    }

    async fn service_realtime(&mut self) -> Result<(), AgentSessionError> {
        while let Some(refresh) = self.actions.take_refresh() {
            let result = async {
                self.navigation = None;
                let input_ids = self.stream.input_ids.clone();
                let info = self.runtime_info().await?;
                self.stream.input_ids.extend(input_ids);
                let handle = self.persistence().ok_or(AgentSessionError::Failed)?;
                let mut journal = history::load(&self.client, &handle, &self.cwd).await?;
                if refresh.action == "navigate_tree" {
                    let native = self.transport.request("get_entries", json!({})).await?;
                    journal.set_navigation(
                        self.navigation_target
                            .take()
                            .as_deref()
                            .ok_or(AgentSessionError::Failed)?,
                        native.get("leafId").ok_or(AgentSessionError::Failed)?,
                    )?;
                    self.navigation.clone_from(&journal.navigation);
                } else {
                    self.navigation_target = None;
                }
                let (history, mut stream) =
                    history::project(&self.client, &handle, journal).await?;
                stream.set_context(&self.state);
                stream.events = std::mem::take(&mut self.stream.events);
                stream.events.push_back(AgentTurnEvent::RuntimeInfo(info));
                stream
                    .events
                    .push_back(AgentTurnEvent::History(history.entries));
                self.stream = stream;
                self.submitted = !self.stream.input_ids.is_empty();
                Ok(())
            }
            .await;
            if let Err(error) = result {
                refresh.finish(Err(error));
                return Err(error);
            }
            self.actions.prepared(refresh);
        }
        Ok(())
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
            self.btw = super::btw::State::default();
            if self.vibe_host.is_some() {
                let spec = AgentSessionSpec {
                    provider: PROVIDER.to_owned(),
                    cwd: self.cwd.clone(),
                    config: self.config.clone(),
                    confinement: self.confinement.clone(),
                };
                self.vibe_host = Some(super::vibe::host::Host::new(
                    &self.client,
                    &spec,
                    id,
                    config::text(&state, "sessionFile")?,
                    None,
                ));
            }
            self.command_controls.close();
            self.created_at = chrono::Utc::now().to_rfc3339();
            self.submitted = false;
            self.navigation = None;
        }
        self.state = state;
        self.stream.set_context(&self.state);
        let mut info = config::runtime(&self.state)?;
        info.mode_id.clone_from(&self.config.mode_id);
        Ok(info)
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        let mut children = self.children.list();
        // Vibe workers arrive through the adapter channel, not OMP's subagent events; the
        // manager validates every published child against this list, so both must be visible.
        for worker in self.vibe.workers.values() {
            // Only workers with a known outcome are publishable: the shared wire status enum
            // has no "unknown", and an unpublished child must not fail the parent's turn.
            if worker.descriptor["status"] != "unknown"
                && !children.iter().any(|child| child.id == worker.id)
            {
                children.push(worker.clone());
            }
        }
        if let Some(host) = &self.vibe_host {
            children.extend(host.children());
        }
        children
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
                ("navigation".to_owned(), json!(self.navigation)),
                (
                    "empty".to_owned(),
                    json!(!self.submitted && !std::path::Path::new(file).is_file()),
                ),
                ("createdAt".to_owned(), json!(self.created_at)),
                (
                    "aitVibeWorkers".to_owned(),
                    self.vibe_host
                        .as_ref()
                        .map_or_else(|| json!([]), super::vibe::host::Host::persisted),
                ),
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
        if let Some(host) = &mut self.vibe_host {
            host.stop();
        }
        self.cancel_requests.insert(id, turn.to_owned());
        self.cancellation_started = Some(Instant::now());
        self.next_state_check = Instant::now();
        Ok(())
    }
    fn pending_permissions(&self) -> Vec<Value> {
        let mut pending = self.dialogs.pending();
        if let Some(host) = &self.vibe_host {
            pending.extend(host.pending());
        }
        pending
    }
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        if let Some(host) = &mut self.vibe_host
            && host.owns_permission(id)
        {
            host.permission(id, response).await?;
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
            return Ok(());
        }
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
        self.drain_vibe()?;
        if let Some(host) = &mut self.vibe_host {
            self.stream
                .events
                .extend(host.poll(self.transport.writer()?));
        }
        if let Some(entry) = self.command_controls.expired() {
            self.stream
                .events
                .push_back(AgentTurnEvent::Timeline(entry));
        }
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
        self.vibe_host.take();
        self.actions.close();
        self.command_controls.close();
        self.stream.events.extend(self.dialogs.clear());
        self.transport.close().await
    }
}

#[cfg(all(test, unix))]
mod tests;
