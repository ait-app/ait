use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use domain::workspace::sandbox::WriteConfinement;
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};
use uuid::Uuid;

use super::{
    LABEL, OmpClient, PROVIDER, commands, config, history,
    interactions::{Captured, Dialogs},
    prompt::Payload,
    roles,
    stream::{self, Stream},
    transport::{Timeouts, Transport},
    usage,
};
use provider_core::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};
use provider_core::ports::diagnostics::{Category, Diagnostic};
use provider_core::protocol::prompt::AgentPrompt;

const STATE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const CANCEL_GRACE: Duration = Duration::from_secs(5);
/// Characters of partial assistant output quoted in a failure, as Paseo quotes.
const FAILURE_PARTIAL_CHARS: usize = 500;
/// Shown when OMP refuses input it had already accepted for writing during a turn.
const STEER_REJECTED: &str =
    "Oh My Pi did not accept a message sent during this turn; it was not delivered.";
/// Wait for OMP to name a held user message's native entry before it is shown with its
/// anchor key instead (D9).
const IDENTITY_GRACE: Duration = Duration::from_secs(2);
/// Shown when Ait cancels an OMP question too large to publish (over 64 KiB).
const DIALOG_DECLINED: &str =
    "Oh My Pi asked a question larger than 64 KiB, which Ait cannot show; it was cancelled.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnPhase {
    Idle,
    AwaitingAdmission,
    Running,
    AwaitingSettlement,
}

/// Whether OMP's todo state should be read again (M4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Todos {
    /// The todo state was read since the last turn ended.
    Current,
    /// A turn ended; its todo state has not been read.
    Stale,
}

/// Whether OMP's state should be read again for its runtime facts (M9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    /// The state was read since OMP last reported `model_changed`.
    Current,
    /// OMP reported `model_changed`; its state has not been read since.
    Changed,
}

/// Progress of the user's cancellation of the active turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cancellation {
    /// No cancellation was requested.
    None,
    /// `abort` was written at this instant and is not yet acknowledged.
    Requested(Instant),
    /// OMP acknowledged the `abort` written at this instant.
    Acknowledged(Instant),
}

impl Cancellation {
    /// When the pending cancellation was written, bounding its confirmation.
    const fn started(self) -> Option<Instant> {
        match self {
            Self::None => None,
            Self::Requested(started) | Self::Acknowledged(started) => Some(started),
        }
    }
}

#[derive(Debug)]
struct StateCheck {
    id: String,
    // New work relinquishes ownership while retaining correlation for the delayed reply.
    turn: Option<String>,
    written: Option<oneshot::Receiver<Result<(), AgentSessionError>>>,
    /// Read after a turn ended, for OMP's todo state rather than settlement (M4).
    after_turn: bool,
    /// Read after OMP reported `model_changed`, for its runtime facts (M9).
    model: bool,
    /// An unanswered check is abandoned, and asked again, after the request timeout.
    deadline: Instant,
}

/// One background `get_branch_messages` request naming held user messages (D9).
#[derive(Debug)]
struct IdentityLookup {
    id: String,
    /// Held user messages announced before the request was written.
    covered: usize,
    written: Option<oneshot::Receiver<Result<(), AgentSessionError>>>,
    deadline: Instant,
}

#[derive(Debug)]
pub(super) struct Session {
    client: OmpClient,
    transport: Transport,
    cwd: String,
    state: Value,
    config: StoredAgentConfig,
    // Restarts relaunch under the same sandbox as the first launch.
    confinement: Option<WriteConfinement>,
    stream: Stream,
    dialogs: Dialogs,
    active: Option<String>,
    closed: bool,
    children: super::subagents::Children,
    prompt_id: Option<String>,
    // An earlier prompt's uncorrelated session_settled can follow the next admission.
    phase: TurnPhase,
    state_check: Option<StateCheck>,
    identity: Option<IdentityLookup>,
    next_state_check: Instant,
    idle_without_outcome: Option<Instant>,
    cancellation: Cancellation,
    cancel_requests: BTreeMap<String, String>,
    input_requests: BTreeMap<String, String>,
    submitted: bool,
    created_at: String,
    /// Session events are live; a resumed process may first replay its session file (M8).
    live: bool,
    todos: Todos,
    usage: usage::Poller,
    /// Built-in slash commands awaiting OMP's answer, by request id (M5).
    pending: BTreeMap<String, slash::Pending>,
    model: Model,
    /// Private role overlay the running process reads; dropped with the process it belongs to.
    overlay: Option<tempfile::NamedTempFile>,
    /// Discovered at open; absent when native metadata could not be read, which only hides roles.
    catalog: Option<std::sync::Arc<roles::Catalog>>,
}

/// Start a disposable native capability reader in `cwd` that saves no session, bounded by
/// `timeouts`. Returns an invalid directory or startup failures.
pub(super) async fn probe(
    client: &OmpClient,
    cwd: &str,
    timeouts: Timeouts,
) -> Result<Transport, AgentSessionError> {
    config::validate_spec(&AgentSessionSpec {
        provider: PROVIDER.to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig::default(),
        confinement: None,
    })?;
    let launch = config::Launch {
        ephemeral: true,
        ..config::Launch::default()
    };
    let args = config::launch_arguments(&StoredAgentConfig::default(), &launch);
    let mut transport = Transport::spawn(client, cwd, &args, None, timeouts)?;
    if let Err(error) = transport.initialize().await {
        let _ = transport.close().await;
        return Err(error);
    }
    Ok(transport)
}

/// Launch one session process for `config` and return it once its state is ready, with the
/// resume model and Fast selection in `launch` applied. A failed start closes the process.
async fn connect(
    client: &OmpClient,
    cwd: &str,
    config: &StoredAgentConfig,
    launch: &config::Launch<'_>,
    confinement: Option<&WriteConfinement>,
) -> Result<(Transport, Value), AgentSessionError> {
    let args = config::launch_arguments(config, launch);
    let timeouts = config::timeouts(config, client.timeouts);
    let mut transport = Transport::spawn(client, cwd, &args, confinement, timeouts)?;
    match prepare(&mut transport, launch).await {
        Ok(state) => Ok((transport, state)),
        Err(error) => {
            let _ = transport.close().await;
            Err(error)
        }
    }
}

async fn prepare(
    transport: &mut Transport,
    launch: &config::Launch<'_>,
) -> Result<Value, AgentSessionError> {
    transport.initialize().await?;
    // OMP sends structured `ask` dialogs only after this; OMP 18.18.8's HTML-form `ask` tool
    // still asks field by field, so a refusal only loses the fast path (O4).
    match transport
        .request("set_ask_dialog", json!({"enabled":true}))
        .await
    {
        Err(AgentSessionError::Rejected) => {
            tracing::debug!(provider = LABEL, "OMP refused set_ask_dialog");
        }
        result => {
            result?;
        }
    }
    subscribe_subagents(transport).await?;
    let mut state = transport.request("get_state", json!({})).await?;
    if let Some(model) = launch.resume_model {
        state = config::resume_model(transport, model, state).await?;
    }
    if let Some(enabled) = launch.fast {
        config::set_fast_mode(transport, &mut state, enabled).await?;
    }
    config::runtime(&state)?;
    Ok(state)
}

/// Subscribe to child session events, falling back to progress only, as Paseo does; an OMP
/// that refuses both still runs, without child timelines (D16).
async fn subscribe_subagents(transport: &mut Transport) -> Result<(), AgentSessionError> {
    for level in ["events", "progress"] {
        match transport
            .request("set_subagent_subscription", json!({"level":level}))
            .await
        {
            Err(AgentSessionError::Rejected) => {
                tracing::debug!(
                    provider = LABEL,
                    level,
                    "OMP refused a subagent subscription"
                );
            }
            result => return result.map(|_| ()),
        }
    }
    Ok(())
}

pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
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
    let session = path.filter(|_| persisted);
    // OMP refuses to start on a missing --model, so a resume keeps the session's model and
    // switches afterwards only to a listed one (D29).
    // Explicit role assignments ride a private overlay read at startup; inherited roles add
    // no file and the native layered settings apply unchanged.
    let overlay = roles::overlay(&spec.config).await?;
    let overlay_path = overlay
        .as_ref()
        .map(|file| file.path().to_str().ok_or(AgentSessionError::Failed))
        .transpose()?;
    let launch = config::Launch {
        overlay: overlay_path,
        model: spec
            .config
            .model
            .as_deref()
            .filter(|_| session.is_none())
            .map(config::model_argument)
            .transpose()?,
        thinking: spec
            .config
            .thinking_option_id
            .as_deref()
            .and_then(config::thinking_level),
        session,
        resume_model: spec.config.model.as_deref().filter(|_| session.is_some()),
        fast: config::requested_fast_mode(&spec.config),
        ..config::Launch::default()
    };
    let (transport, state) = connect(
        client,
        &spec.cwd,
        &spec.config,
        &launch,
        spec.confinement.as_ref(),
    )
    .await?;
    let id = config::text(&state, "sessionId")?;
    if persisted && handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    let mut stream = replayed.unwrap_or_else(|| Stream::new(id.to_owned(), client.images.clone()));
    stream = stream
        .with_blobs(history::paths::blobs(client, Some(&spec.cwd))?)
        .with_identity_lookups();
    // OMP's todo state at attach, unless the replayed history already shows it (M4).
    stream.state_todos(&state);
    Ok(Session {
        client: client.clone(),
        transport,
        cwd: spec.cwd.clone(),
        state,
        config: spec.config.clone(),
        confinement: spec.confinement.clone(),
        stream,
        dialogs: Dialogs::default(),
        active: None,
        closed: false,
        children: super::subagents::Children::default()
            .with_blobs(history::paths::blobs(client, Some(&spec.cwd))?),
        prompt_id: None,
        phase: TurnPhase::Idle,
        state_check: None,
        identity: None,
        next_state_check: Instant::now(),
        idle_without_outcome: None,
        cancellation: Cancellation::None,
        cancel_requests: BTreeMap::new(),
        input_requests: BTreeMap::new(),
        submitted: false,
        created_at,
        live: !persisted,
        todos: Todos::Current,
        usage: usage::Poller::default(),
        pending: BTreeMap::new(),
        model: Model::Current,
        overlay,
        catalog: client.role_catalog(&spec.cwd).await.ok(),
    })
}

impl Session {
    /// Catalog discovered with this session; `None` when the native read failed.
    fn catalog(&self) -> Option<&roles::Catalog> {
        self.catalog.as_deref()
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
        self.validate_roles(config).await?;
        // Mode, system prompt, provider options and role assignments are launch arguments
        // (M10). An overlay change needs a process that reads the new file at startup.
        if self.config.mode_id == config.mode_id
            && self.config.system_prompt == config.system_prompt
            && self.config.provider_options == config.provider_options
            && roles::same_overlay(&self.config, config)
        {
            self.state = config::apply(&mut self.transport, &self.config, config).await?;
            self.config.clone_from(config);
            return Ok(());
        }
        self.restart(config).await
    }

    /// Relaunch OMP for a new mode, system prompt or provider options, as Paseo relaunches it
    /// for a mode: start and read the new process before closing the old one, carrying model,
    /// thinking and Fast across. A failed start leaves the current process in place.
    async fn restart(&mut self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        // A command in flight, such as a compaction, belongs to the running process.
        if !self.pending.is_empty() {
            return Err(AgentSessionError::Rejected);
        }
        // The old process may have changed its model or Fast since the last read.
        if let Ok(state) = self.transport.request("get_state", json!({})).await
            && state["sessionId"] == self.state["sessionId"]
        {
            self.state = state;
        }
        let path = config::text(&self.state, "sessionFile")?.to_owned();
        let persisted = std::path::Path::new(&path).is_file();
        // Materialized before launch so the new process can read it, and kept until that
        // process is replaced; inherited roles create no file.
        let overlay = roles::overlay(config).await?;
        let overlay_path = overlay
            .as_ref()
            .map(|file| file.path().to_str().ok_or(AgentSessionError::Failed))
            .transpose()?;
        let mut launch = self.carried_launch(config, persisted.then_some(path.as_str()))?;
        launch.overlay = overlay_path;
        let (mut transport, state) = connect(
            &self.client,
            &self.cwd,
            config,
            &launch,
            self.confinement.as_ref(),
        )
        .await?;
        if persisted && state["sessionId"] != self.state["sessionId"] {
            let _ = transport.close().await;
            return Err(AgentSessionError::Failed);
        }
        let mut old = std::mem::replace(&mut self.transport, transport);
        if old.close().await.is_err() {
            tracing::warn!("the replaced OMP process did not exit after SIGKILL");
        }
        // The replaced process cannot answer an identity lookup.
        self.release_identities();
        if !persisted {
            self.stream = Stream::new(
                config::text(&state, "sessionId")?.to_owned(),
                self.client.images.clone(),
            )
            .with_blobs(history::paths::blobs(&self.client, Some(&self.cwd))?)
            .with_identity_lookups();
        }
        // Request ids restart with each process, so nothing held can correlate with it.
        self.prompt_id = None;
        self.stream.pending_inputs.clear();
        self.state_check = None;
        self.usage.reset();
        self.cancel_requests.clear();
        self.input_requests.clear();
        self.idle_without_outcome = None;
        self.live = !persisted;
        self.state = state;
        self.stream.state_todos(&self.state);
        self.config.clone_from(config);
        // Installed after the old process is gone, so its overlay outlives exactly one reader.
        self.overlay = overlay;
        // Same cwd/environment identity: a cached catalog still describes this profile, and a
        // failed read keeps the previous one rather than hiding role controls.
        if let Ok(catalog) = self.client.role_catalog(&self.cwd).await {
            self.catalog = Some(catalog);
        }
        Ok(())
    }

    /// Launch selections for a restart into `config`: changed selections win, otherwise the
    /// running process's model, thinking level and Fast carry across.
    fn carried_launch<'a>(
        &self,
        config: &'a StoredAgentConfig,
        session: Option<&'a str>,
    ) -> Result<config::Launch<'a>, AgentSessionError> {
        let model = match config
            .model
            .as_deref()
            .filter(|_| config.model != self.config.model)
        {
            Some(route) => Some(config::model_argument(route)?),
            None => config::state_model(&self.state)?,
        };
        let thinking = config
            .thinking_option_id
            .as_deref()
            .filter(|_| config.thinking_option_id != self.config.thinking_option_id)
            .or_else(|| self.state["thinkingLevel"].as_str())
            .and_then(config::thinking_level);
        let fast = config::requested_fast_mode(config)
            .filter(|_| {
                config::requested_fast_mode(config) != config::requested_fast_mode(&self.config)
            })
            .or_else(|| config::fast_mode(&self.state).map(|fast| fast.enabled));
        Ok(config::Launch {
            model,
            thinking,
            session,
            fast,
            ..config::Launch::default()
        })
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
        let payload = Self::payload(prompt)?;
        self.submit(payload, None, prompt.client_message_id.clone())
            .await
    }

    /// Convert `prompt` for OMP (D7).
    ///
    /// # Errors
    ///
    /// Returns malformed or oversized prompt errors.
    fn payload(prompt: &AgentPrompt) -> Result<Payload, AgentSessionError> {
        Ok(Payload::new(prompt.blocks()?))
    }

    /// Write `payload` as OMP's prompt for a new turn, with `streaming_behavior` for `/steer`
    /// and `/follow-up`, and track the turn until OMP settles it.
    async fn submit(
        &mut self,
        payload: Payload,
        streaming_behavior: Option<&str>,
        client_message_id: Option<String>,
    ) -> Result<String, AgentSessionError> {
        self.transport.diagnostics.clear();
        let preview = stream::preview(&payload.text);
        let prompt_id = self
            .transport
            .begin("prompt", payload.fields(streaming_behavior))
            .await?;
        self.stream
            .pending_inputs
            .push_back((prompt_id.clone(), client_message_id));
        let turn = Uuid::new_v4().to_string();
        self.stream.begin(turn.clone(), Some(preview));
        self.active = Some(turn.clone());
        self.prompt_id = Some(prompt_id);
        self.phase = TurnPhase::AwaitingAdmission;
        self.cancellation = Cancellation::None;
        // Paseo treats a local prompt as the end of any replay.
        self.live = true;
        self.next_state_check = Instant::now() + STATE_CHECK_INTERVAL;
        self.idle_without_outcome = None;
        self.usage.start_turn();
        Ok(turn)
    }

    fn prompt_admitted(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if event["command"] != "prompt" {
            return Err(AgentSessionError::Failed);
        }
        match event["success"].as_bool() {
            // COMPAT (OMP 18.3; 18.18.8 still sends it before its own failed prompt_result):
            // a prompt that fails after admission is answered a second time with a failure.
            Some(false) if self.phase != TurnPhase::AwaitingAdmission => {
                self.prompt_result(
                    &json!({"status":"error","agentInvoked":false,"error":event["error"]}),
                );
            }
            Some(false) => {
                self.transport
                    .diagnostics
                    .observe_native(&event["error"], "rpc", "prompt");
                self.stream.failed = true;
                self.finish();
            }
            Some(true) if self.phase == TurnPhase::AwaitingAdmission => {
                self.phase = TurnPhase::Running;
                if event["data"]["agentInvoked"] == false {
                    // OMP 18.18.8 sends no prompt_result for a local command (D8), so this
                    // turn settles through the result or native idle state, checked now.
                    if let Some(prompt) = event["id"].as_str() {
                        self.stream.handled_locally(prompt);
                    }
                    self.phase = TurnPhase::AwaitingSettlement;
                    self.next_state_check = Instant::now();
                }
            }
            Some(true) => {}
            None => return Err(AgentSessionError::Failed),
        }
        Ok(())
    }

    /// Record the active prompt's correlated outcome; it finishes once OMP has settled.
    fn prompt_result(&mut self, event: &Value) {
        if event["agentInvoked"] == false
            && let Some(prompt) = self.prompt_id.clone()
        {
            self.stream.handled_locally(&prompt);
        }
        self.stream.failed = event["status"] == "error";
        // An assistant failure already explained this turn more completely.
        if self.stream.failed && self.transport.diagnostics.message().is_none() {
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

    /// Record an assistant message that ended in error with Paseo's failure detail (D27).
    /// The category comes from OMP's reason alone, never from the quoted partial output.
    fn observe_assistant_failure(&self, message: &Value) {
        let reason = message["errorMessage"]
            .as_str()
            .map(str::trim)
            .filter(|reason| !reason.is_empty())
            .unwrap_or("OMP turn failed");
        let diagnostic = provider_native::diagnostics::native(
            &json!({"message":reason,"status":message["errorStatus"]}),
            "message_end",
        );
        self.transport.diagnostics.observe_failure(
            diagnostic,
            &failure_text(reason, message),
            "turn",
        );
    }

    fn control_response(&mut self, event: &Value) -> Result<bool, AgentSessionError> {
        if self.usage.owns(event) {
            if let Some(usage) = self.usage.answer(event, self.stream.totals()) {
                self.publish_usage(usage);
            }
            return Ok(true);
        }
        if let Some(id) = event["id"].as_str()
            && let Some(pending) = self.pending.remove(id)
        {
            self.command_answered(pending, event);
            return Ok(true);
        }
        if let Some(lookup) = self
            .identity
            .take_if(|lookup| event["id"] == lookup.id.as_str())
        {
            let messages = (event["success"] == true).then(|| &event["data"]["messages"]);
            self.stream.resolve_identities(lookup.covered, messages);
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
            // A steer, or a `/steer` or `/follow-up` prompt queued during the turn.
            if matches!(event["command"].as_str(), Some("steer" | "prompt"))
                && event["success"] == true
            {
                return Ok(true);
            }
            self.stream
                .pending_inputs
                .retain(|(request, _)| request != id);
            if self.active.as_deref() == Some(&turn) {
                self.steer_rejected(&event["error"]);
            }
            return Ok(true);
        }
        if let Some(id) = event["id"].as_str()
            && let Some(turn) = self.cancel_requests.remove(id)
        {
            if self.active.as_deref() != Some(&turn) {
                return Ok(true);
            }
            if event["command"] != "abort" || event["success"] != true {
                self.transport
                    .diagnostics
                    .observe_native(&event["error"], "rpc", "abort");
                return Err(AgentSessionError::Failed);
            }
            // `abort` keeps OMP's queued steering, so pending inputs still await their echo.
            if let Cancellation::Requested(started) = self.cancellation {
                self.cancellation = Cancellation::Acknowledged(started);
            }
            self.stream.aborted = true;
            self.phase = TurnPhase::AwaitingSettlement;
            self.next_state_check = Instant::now();
            return Ok(true);
        }
        Ok(false)
    }

    /// Report a steering message OMP refused after its write was accepted (D6). The turn
    /// keeps running: the port cannot hand an accepted input back to the runtime's queue.
    fn steer_rejected(&mut self, error: &Value) {
        tracing::warn!(
            provider = LABEL,
            "OMP rejected steering input after accepting its write"
        );
        let text = native_reason(error, "steer").map_or_else(
            || STEER_REJECTED.to_owned(),
            |reason| format!("{STEER_REJECTED}\n{reason}"),
        );
        self.stream.notify("warning", text);
    }

    /// Publish a changed usage snapshot before any turn outcome still queued, as Paseo
    /// publishes a turn's final usage before its completion.
    fn publish_usage(&mut self, usage: provider_core::protocol::usage::AgentUsage) {
        let events = &mut self.stream.events;
        let position = events.iter().position(is_outcome).unwrap_or(events.len());
        events.insert(position, AgentTurnEvent::Usage(usage));
    }

    /// Start a host turn for native work Ait did not submit, such as a resumed queue.
    fn begin_autonomous(&mut self) {
        let turn = Uuid::new_v4().to_string();
        self.stream.begin(turn.clone(), None);
        self.stream.start_run();
        self.active = Some(turn.clone());
        self.phase = TurnPhase::AwaitingSettlement;
        self.next_state_check = Instant::now() + STATE_CHECK_INTERVAL;
        self.usage.start_turn();
        self.stream.events.push_back(AgentTurnEvent::Started(turn));
    }

    /// Handle `agent_start`: start an autonomous turn, mark the active turn's run started,
    /// or, after an acknowledged `abort`, end the cancelled turn before OMP's queued run.
    fn run_started(&mut self) {
        if self.active.is_none() {
            self.begin_autonomous();
        } else if matches!(self.cancellation, Cancellation::Acknowledged(_)) {
            // OMP 18.18.8 keeps steering and follow-up messages through `abort` and runs
            // them at once in a new run. The cancelled prompt is over, so that run gets its
            // own turn and the steering inputs keep their client identities.
            let prompt = self.prompt_id.clone();
            let mut queued = std::mem::take(&mut self.stream.pending_inputs);
            queued.retain(|(request, _)| Some(request) != prompt.as_ref());
            self.finish();
            self.begin_autonomous();
            self.stream.pending_inputs = queued;
        } else {
            self.stream.start_run();
        }
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if event["type"] == "response" && self.control_response(event)? {
            return Ok(());
        }
        if !self.live {
            if event["type"] == "agent_start" {
                self.live = true;
            } else if is_session_event(event) {
                // Pre-existing conversation is shown from history, never as live output.
                return Ok(());
            }
        }
        if event["type"] == "agent_start" {
            self.invalidate_state_check();
        }
        if event["type"] == "message_end" && event["message"]["role"] == "assistant" {
            self.submitted = true;
            if event["message"]["stopReason"] == "error" {
                self.observe_assistant_failure(&event["message"]);
            } else {
                self.transport.diagnostics.clear();
            }
        }
        self.dialogs.observe(event);
        match event["type"].as_str() {
            Some("extension_ui_request") => match self.dialogs.capture(event)? {
                Captured::Ignored => self.project(event)?,
                Captured::Events(events) => self.stream.events.extend(events),
                Captured::Reply(reply) => self.transport.reply(reply)?,
                Captured::Declined(reply) => {
                    self.transport.reply(reply)?;
                    self.stream.notify("warning", DIALOG_DECLINED.to_owned());
                }
            },
            Some("agent_start") => self.run_started(),
            Some("command_output") => {
                // Output outside a turn has no prompt to belong to (D12).
                if self.active.is_some() {
                    let result = self.stream.command_output(event);
                    self.projected(result)?;
                }
            }
            Some("subagent_lifecycle" | "subagent_progress" | "subagent_event") => {
                self.subagent(event)?;
            }
            // OMP names no model; its state is read in the background (M9). The context
            // window may have changed with it.
            Some("model_changed") => {
                self.model = Model::Changed;
                self.usage.refresh();
            }
            Some("response") if self.prompt_id.as_deref() == event["id"].as_str() => {
                self.prompt_admitted(event)?;
            }
            Some("prompt_result") if self.prompt_id.as_deref() == event["id"].as_str() => {
                self.prompt_result(event);
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
            Some("host_tool_call" | "host_uri_request") => {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "host_request"),
                    "OMP requested a host tool or URI handler that Ait did not register.",
                    "turn",
                );
                return Err(AgentSessionError::Failed);
            }
            // Like Paseo, an uncorrelated response, even a failed one, belongs to no turn.
            Some("response") => {
                tracing::debug!(provider = LABEL, "Ignoring an uncorrelated OMP response");
            }
            _ => self.project(event)?,
        }
        Ok(())
    }

    fn project(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let result = self.stream.consume(event);
        self.projected(result)
    }

    /// Track a child of this session and name the `task` card that started it (D16).
    fn subagent(&mut self, frame: &Value) -> Result<(), AgentSessionError> {
        let parent = super::subagents::Parent {
            id: config::text(&self.state, "sessionId")?,
            file: self.state["sessionFile"].as_str().map(std::path::Path::new),
            cwd: &self.cwd,
        };
        let events = self.children.consume(frame, parent, &self.client.images)?;
        self.stream.events.extend(events);
        let payload = &frame["payload"];
        let (child, description) = if frame["type"] == "subagent_progress" {
            (&payload["progress"], &payload["progress"]["description"])
        } else {
            (payload, &payload["description"])
        };
        if frame["type"] != "subagent_event"
            && let (Some(call), Some(job)) =
                (payload["parentToolCallId"].as_str(), child["id"].as_str())
        {
            let index = payload["index"]
                .as_u64()
                .or_else(|| child["index"].as_u64())
                .unwrap_or_default();
            self.stream
                .task_child(call, index, job, description.as_str());
        }
        Ok(())
    }

    /// Turn a projection failure into a payload-free turn failure.
    fn projected(&self, result: Result<(), stream::Error>) -> Result<(), AgentSessionError> {
        result.map_err(|error| {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "event"),
                &error.to_string(),
                "project",
            );
            AgentSessionError::Failed
        })
    }

    /// End the active turn with its recorded outcome. A local prompt first gets its echo and
    /// held output (D8). Tools left without a result are closed; a cancelled or failed turn
    /// also closes its running children.
    fn finish(&mut self) {
        if self.active.take().is_none() {
            return;
        }
        self.stream.settle_prompt();
        let outcome = if self.stream.aborted {
            AgentTurnEvent::Cancelled
        } else if self.stream.failed {
            AgentTurnEvent::Failed
        } else {
            AgentTurnEvent::Completed(self.stream.last_text.clone())
        };
        // A result cannot follow once the turn settled, even after a completed turn.
        self.stream.terminalize();
        if !matches!(outcome, AgentTurnEvent::Completed(_)) {
            let children = self.children.terminalize();
            self.stream.events.extend(children);
        }
        self.prompt_id = None;
        self.stream.pending_inputs.clear();
        self.phase = TurnPhase::Idle;
        self.cancellation = Cancellation::None;
        self.idle_without_outcome = None;
        self.todos = Todos::Stale;
        self.usage.complete_turn();
        self.stream.events.extend(self.dialogs.clear());
        self.stream.events.push_back(outcome);
    }

    /// End the active turn as failed after its connection or protocol failed (D28).
    fn fail_turn(&mut self) {
        self.release_identities();
        self.stream.aborted = false;
        self.stream.failed = true;
        self.finish();
        // The connection that would answer a final usage read is gone.
        self.usage.stop();
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
        // An extension command can start or switch OMP's session; follow it, as a read
        // through `runtime_info` does.
        if event["success"] == true && event["data"]["sessionId"] != self.state["sessionId"] {
            self.adopt_session(event["data"].clone())?;
            // A read after `model_changed` publishes the facts itself.
            if !check.model
                && let Ok(info) = self.runtime()
            {
                self.stream
                    .events
                    .push_back(AgentTurnEvent::RuntimeInfo(info));
            }
        }
        if check.model {
            self.model_state(event);
        }
        if check.after_turn {
            // A todo tool result of new work would show the same list again.
            if self.active.is_none()
                && event["success"] == true
                && event["data"]["sessionId"] == self.state["sessionId"]
            {
                self.stream.state_todos(&event["data"]);
            }
            return Ok(());
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
        if self.cancellation != Cancellation::None {
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

    /// Adopt the state read after `model_changed` and publish its runtime facts, as Paseo
    /// does (M9). A refused or foreign read leaves the last state in place.
    fn model_state(&mut self, event: &Value) {
        let state = &event["data"];
        if event["success"] != true || state["sessionId"] != self.state["sessionId"] {
            tracing::debug!(
                provider = LABEL,
                "OMP state unavailable after a model change"
            );
            return;
        }
        let previous = std::mem::replace(&mut self.state, state.clone());
        if let Ok(info) = self.runtime() {
            self.stream
                .events
                .push_back(AgentTurnEvent::RuntimeInfo(info));
        } else {
            tracing::debug!(
                provider = LABEL,
                "OMP reported an invalid model after a change"
            );
            self.state = previous;
        }
    }

    /// Follow OMP to the native session `state` names: its history, children and identity
    /// lookups start afresh, while queued events and the active turn carry across. The
    /// previous session's open tools are closed, as their results cannot arrive.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` for a session id that is no UUID.
    fn adopt_session(&mut self, state: Value) -> Result<(), AgentSessionError> {
        let id = config::text(&state, "sessionId")?;
        Uuid::parse_str(id).map_err(|_| AgentSessionError::Failed)?;
        let blobs = history::paths::blobs(&self.client, Some(&self.cwd))?;
        self.release_identities();
        self.stream.terminalize();
        self.stream.switch_session(id.to_owned());
        self.children = super::subagents::Children::default().with_blobs(blobs);
        self.created_at = chrono::Utc::now().to_rfc3339();
        self.submitted = false;
        self.state = state;
        Ok(())
    }

    /// Runtime facts from the last native state, with the effective mode: none means Full.
    fn runtime(&self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        let mut info = config::runtime(&self.state)?;
        info.mode_id = Some(
            self.config
                .mode_id
                .clone()
                .unwrap_or_else(|| "full".to_owned()),
        );
        Ok(info)
    }

    fn check_cancellation(&self) -> Result<(), AgentSessionError> {
        if self.cancellation.started().is_some_and(|started| {
            started.elapsed() >= CANCEL_GRACE.min(self.transport.timeouts().request)
        }) {
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
        if let Some(check) = self
            .state_check
            .take_if(|check| Instant::now() >= check.deadline)
        {
            // A lost or unanswered read must not stop settlement checks; its late answer is
            // an uncorrelated response. What it was asked for is asked again.
            tracing::debug!(provider = LABEL, "OMP did not answer a state check in time");
            if check.after_turn {
                self.todos = Todos::Stale;
            }
            if check.model {
                self.model = Model::Changed;
            }
        }
        if self.state_check.is_some() {
            return Ok(());
        }
        // After a turn, read OMP's todo state once, as Paseo shows it with a live history.
        let after_turn = self.active.is_none() && self.todos == Todos::Stale && !self.closed;
        let model = self.model == Model::Changed && !self.closed;
        let turn = self.active.as_ref().filter(|_| {
            matches!(
                self.phase,
                TurnPhase::Running | TurnPhase::AwaitingSettlement
            )
        });
        if !after_turn && !model && (turn.is_none() || Instant::now() < self.next_state_check) {
            return Ok(());
        }
        let (id, written) = match self.transport.background("get_state") {
            Ok(check) => check,
            Err(AgentSessionError::Busy) => return Ok(()),
            Err(error) => return Err(error),
        };
        if after_turn {
            self.todos = Todos::Current;
        }
        if model {
            self.model = Model::Current;
        }
        self.state_check = Some(StateCheck {
            id,
            turn: turn.cloned(),
            written: Some(written),
            after_turn,
            model,
            deadline: Instant::now() + self.transport.timeouts().request,
        });
        Ok(())
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn reports_token_usage(&self) -> bool {
        true
    }

    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        Ok(())
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        // A selection already applied shows OMP's answer, which may have refused it.
        let requested = config::requested_fast_mode(config);
        let pending = requested.filter(|_| requested != config::requested_fast_mode(&self.config));
        let mut features = config::features(pending, Some(&self.state));
        // Role assignments are catalog data; a missing catalog renders stored keys only.
        features.extend(match self.catalog() {
            Some(catalog) => catalog.features(config),
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
        Some(Cow::Owned(format!(
            "{LABEL}: {}",
            diagnostic.category.message()
        )))
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        // The manager asks immediately after submission, before polling any dialogs.
        // A queued get_state can wait on that same dialog and prevent its publication.
        if self.active.is_some() {
            return self.runtime();
        }
        let state = self.transport.request("get_state", json!({})).await?;
        if state["sessionId"] == self.state["sessionId"] {
            self.state = state;
        } else {
            self.adopt_session(state)?;
        }
        self.runtime()
    }
    fn subagents(&self) -> Vec<provider_core::ports::controls::NativeSubagent> {
        self.children.list()
    }
    async fn out_of_band(&mut self, prompt: &AgentPrompt) -> Result<(), AgentSessionError> {
        self.run_command(prompt).await
    }
    fn pending_foreground(&self) -> bool {
        // An autonomous native run started, but the runtime has not seen its turn yet.
        self.stream
            .events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Started(_)))
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
        let payload = Self::payload(prompt)?;
        // OMP queues steered slash text as a literal message instead of running the command,
        // so the runtime delivers it as its own prompt instead (D6).
        if is_slash_command(&payload.text) {
            return Err(AgentSessionError::Rejected);
        }
        let id = self.transport.begin("steer", payload.fields(None)).await?;
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
        // As in Paseo, an exited process counts as stopped; polling reports its exit.
        if self.cancellation != Cancellation::None || self.transport.exited() {
            return Ok(());
        }
        if self.cancel_requests.len() >= 128 {
            return Err(AgentSessionError::Failed);
        }
        // Paseo's `abort`: unlike `abort_and_restore_queue` it keeps OMP's queued steering.
        let id = self.transport.begin("abort", json!({})).await?;
        self.cancel_requests.insert(id, turn.to_owned());
        self.cancellation = Cancellation::Requested(Instant::now());
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
        let answer = self.dialogs.response(id, response)?;
        self.transport.send(&answer.native).await?;
        self.dialogs.resolve(id, answer);
        self.stream
            .events
            .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
        Ok(())
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        match self.observe() {
            // The turn cannot outlive its connection: close its unfinished work, then report
            // the retained reason, such as the exit text, as the turn's failure (D16, D28).
            Err(error) if self.active.is_some() => {
                self.fail_turn();
                Ok(Some(self.stream.events.pop_front().ok_or(error)?))
            }
            result => result,
        }
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.closed = true;
        self.usage.stop();
        self.pending.clear();
        self.release_identities();
        self.stream.events.extend(self.dialogs.clear());
        self.transport.close().await
    }
}

impl Session {
    /// Return the next turn event, reading native frames until one is produced.
    fn observe(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        self.check_cancellation()?;
        if let Some(event) = self.next_event() {
            return Ok(Some(event));
        }
        for _ in 0..64 {
            let Some(event) = self.transport.poll()? else {
                self.poll_identity();
                self.poll_state_check()?;
                let limit = self.transport.timeouts().request;
                self.usage.poll(&mut self.transport, limit);
                return Ok(self.next_event());
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
            self.poll_identity();
            if let Some(event) = self.next_event() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    /// The next event to publish; a turn's outcome waits briefly for its final usage read.
    fn next_event(&mut self) -> Option<AgentTurnEvent> {
        if self.usage.holds_outcome() && self.stream.events.front().is_some_and(is_outcome) {
            return None;
        }
        self.stream.next_event()
    }

    /// Name held user messages by their native entries (D9) with one background
    /// `get_branch_messages` at a time, so neither event polling nor the caller waits on it.
    /// A failed write or an elapsed grace shows the covered messages with their anchor keys.
    fn poll_identity(&mut self) {
        if let Some(lookup) = &mut self.identity {
            let failed = match lookup.written.as_mut().map(oneshot::Receiver::try_recv) {
                Some(Ok(Ok(()))) => {
                    lookup.written = None;
                    false
                }
                None | Some(Err(oneshot::error::TryRecvError::Empty)) => false,
                Some(Ok(Err(_)) | Err(oneshot::error::TryRecvError::Closed)) => true,
            };
            if !failed && Instant::now() < lookup.deadline {
                return;
            }
            let covered = lookup.covered;
            self.identity = None;
            tracing::debug!(
                provider = LABEL,
                "OMP did not name a user message's native entry in time"
            );
            self.stream.resolve_identities(covered, None);
        }
        let waiting = self.stream.unresolved();
        if waiting == 0 {
            return;
        }
        match self.transport.background("get_branch_messages") {
            Ok((id, written)) => {
                self.identity = Some(IdentityLookup {
                    id,
                    covered: waiting,
                    written: Some(written),
                    deadline: Instant::now()
                        + IDENTITY_GRACE.min(self.transport.timeouts().request),
                });
            }
            // A state check is being written; the next poll asks again.
            Err(AgentSessionError::Busy) => {}
            Err(_) => self.stream.resolve_identities(waiting, None),
        }
    }

    /// Show every held user message with its anchor key and forget the pending lookup,
    /// whose reply can no longer arrive or no longer applies.
    fn release_identities(&mut self) {
        self.identity = None;
        let held = self.stream.unresolved();
        self.stream.resolve_identities(held, None);
    }
}

/// Paseo's session events, which a resumed process may replay before new work (M8).
fn is_session_event(event: &Value) -> bool {
    matches!(
        event["type"].as_str(),
        Some(
            "agent_start"
                | "turn_start"
                | "message_start"
                | "message_update"
                | "message_end"
                | "tool_execution_start"
                | "tool_execution_update"
                | "tool_execution_end"
                | "compaction_start"
                | "compaction_end"
                | "agent_end"
        )
    )
}

/// Whether `text` invokes a slash command, by Paseo's rule: `/name` with an optional
/// argument, where the name contains no further `/`.
fn is_slash_command(text: &str) -> bool {
    commands::split(text).is_some()
}

/// Whether `event` ends a turn.
fn is_outcome(event: &AgentTurnEvent) -> bool {
    matches!(
        event,
        AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
    )
}

/// OMP's refusal reason, bounded and redacted by separate diagnostics so that it never
/// fails the session.
fn native_reason(error: &Value, operation: &str) -> Option<String> {
    let reason = provider_native::diagnostics::Diagnostics::new(LABEL);
    reason.observe_native(error, "rpc", operation);
    reason.message()
}

/// Paseo's failure text: OMP's `reason`, then stopReason, model, responseModel, responseId
/// and up to 500 characters of the message's partial text. Diagnostics bound and redact it.
fn failure_text(reason: &str, message: &Value) -> String {
    let field = |name: &str| message[name].as_str().filter(|value| !value.is_empty());
    let mut details = Vec::new();
    if let Some(stop) = field("stopReason") {
        details.push(format!("stopReason={stop}"));
    }
    if let (Some(provider), Some(model)) = (field("provider"), field("model")) {
        details.push(format!("model={provider}/{model}"));
    }
    if let Some(model) = field("responseModel") {
        details.push(format!("responseModel={model}"));
    }
    if let Some(id) = field("responseId") {
        details.push(format!("responseId={id}"));
    }
    let partial = message["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|block| match block["type"].as_str() {
            Some("text") => block["text"].as_str(),
            Some("thinking") => block["thinking"].as_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let partial = partial.trim();
    if !partial.is_empty() {
        let quoted: String = partial.chars().take(FAILURE_PARTIAL_CHARS).collect();
        details.push(format!("partial={}", json!(quoted)));
    }
    if details.is_empty() {
        reason.to_owned()
    } else {
        format!("{reason} ({})", details.join(", "))
    }
}

mod slash;

#[cfg(all(test, unix))]
mod tests;
