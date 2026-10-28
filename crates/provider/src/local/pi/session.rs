use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    PROVIDER, PiClient, config,
    controls::{DeferredConfig, Intent, Plan, ResetIntent, Selection, SuppressedControls},
    streaming::Stream,
    transport::{self, Transport},
};
use crate::ports::agent_session::{
    AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec,
    AgentTurnEvent, ConfigApplication,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
use crate::protocol::prompt::AgentPrompt;

const ABORT_SETTLEMENT_TIMEOUT: Duration = Duration::from_secs(10);

/// One owned native writer and its stable resume identity.
#[derive(Debug)]
pub(super) struct Session {
    transport: Transport,
    id: String,
    cwd: String,
    native_cwd: String,
    state: Value,
    runtime_changed: bool,
    config: StoredAgentConfig,
    // Startup defaults cannot confirm preferences whose restoration was suppressed.
    suppressed: SuppressedControls,
    reset: ResetIntent,
    initialization_error: Option<AgentSessionError>,
    deferred: Option<DeferredConfig>,
    images: crate::local::images::ImageStore,
    active: Option<String>,
    stream: Stream,
    has_input: bool,
    closed: bool,
}

/// Open `spec` as a fresh session or resume the verified `handle`.
/// Returns rejection for foreign/missing history and failure for native protocol errors.
pub(super) async fn open(
    client: &PiClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
    purpose: AgentResumePurpose,
) -> Result<Session, AgentSessionError> {
    config::spec(spec)?;
    let id = handle.map_or_else(
        || Uuid::new_v4().to_string(),
        |handle| handle.session_id.clone(),
    );
    let has_input = handle
        .and_then(|handle| handle.metadata.as_ref())
        .is_some_and(|data| data.get("hasInput") == Some(&Value::Bool(true)));
    let session_directory = match super::history::session_directory(client, Path::new(&spec.cwd)) {
        Ok(directory) => Some(directory),
        Err(AgentSessionError::Unavailable) => None,
        Err(error) => return Err(error),
    };
    let mut args = vec![OsString::from("--session-id"), OsString::from(&id)];
    if let Some(handle) = handle {
        if handle.provider != PROVIDER
            || Uuid::parse_str(&id).is_err()
            || handle
                .metadata
                .as_ref()
                .and_then(|data| data.get("cwd"))
                .and_then(Value::as_str)
                != Some(spec.cwd.as_str())
        {
            return Err(AgentSessionError::Rejected);
        }
        if has_input {
            let path = handle
                .native_handle
                .as_ref()
                .and_then(Value::as_str)
                .or_else(|| handle.metadata.as_ref()?.get("sessionFile")?.as_str())
                .ok_or(AgentSessionError::Rejected)?;
            if !Path::new(path).is_absolute() || !Path::new(path).is_file() {
                return Err(AgentSessionError::Rejected);
            }
            args = vec![OsString::from("--session"), OsString::from(path)];
        }
    }
    if let Some(session_directory) = session_directory {
        args.push("--session-dir".into());
        args.push(session_directory.into_os_string());
    }
    if !has_input && matches!(purpose, AgentResumePurpose::Configuration { .. }) {
        // Configuration must not create native history before the first input.
        args.push("--no-session".into());
    }
    transport::version(client).await?;
    let deferred: Option<DeferredConfig> = handle
        .and_then(|handle| handle.metadata.as_ref()?.get("deferredConfig"))
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| AgentSessionError::Rejected))
        .transpose()?;
    let mut session = Session {
        transport: Transport::spawn(client, &spec.cwd, &args)?,
        id,
        cwd: spec.cwd.clone(),
        native_cwd: handle
            .and_then(|handle| handle.metadata.as_ref()?.get("nativeCwd")?.as_str())
            .unwrap_or(&spec.cwd)
            .to_owned(),
        state: Value::Null,
        runtime_changed: false,
        config: deferred
            .as_ref()
            .and_then(|pending| pending.previous_config.clone())
            .unwrap_or_else(|| spec.config.clone()),
        suppressed: SuppressedControls::default(),
        reset: if !has_input
            && deferred
                .as_ref()
                .is_some_and(DeferredConfig::clears_thinking)
        {
            ResetIntent::UntilInput
        } else {
            ResetIntent::None
        },
        initialization_error: None,
        deferred,
        images: client.images.clone(),
        active: None,
        stream: Stream::default(),
        has_input,
        closed: false,
    };
    let state = session.request(json!({"type":"get_state"}), false).await?;
    session.verify_session_identity(&state).await?;
    if state["isStreaming"] != false || state["isCompacting"] != false {
        return Err(AgentSessionError::Failed);
    }
    session.state = state;
    session.initialize_controls(&spec.config, purpose).await?;
    Ok(session)
}

impl Session {
    async fn initialize_controls(
        &mut self,
        config: &StoredAgentConfig,
        purpose: AgentResumePurpose,
    ) -> Result<(), AgentSessionError> {
        if let AgentResumePurpose::Configuration {
            replace_model,
            replace_thinking,
        } = purpose
        {
            return self
                .reconcile_open(replace_model, replace_thinking || replace_model)
                .await;
        }
        let deferred = self.deferred.is_some();
        match self.configure_open(config).await {
            Err(AgentSessionError::Rejected) if deferred && self.deferred.is_none() => {
                // Return the owned session so registration can save consumed intent and
                // partial native success before reporting the definitive rejection.
                self.initialization_error = Some(AgentSessionError::Rejected);
                Ok(())
            }
            result => result,
        }
    }
    async fn reconcile_open(
        &mut self,
        replace_model: bool,
        replace_thinking: bool,
    ) -> Result<(), AgentSessionError> {
        // A never-prompted session starts with native defaults under --session-id.
        // Restore untouched preferences before registration observes those defaults.
        // Deferred commands remain owned by apply_config, where the patch supersedes them.
        let pending = self.deferred.as_ref().map(DeferredConfig::patch);
        let config = self.config.clone();
        self.suppressed = SuppressedControls {
            model: replace_model,
            thinking: replace_thinking,
        };
        self.configure(
            &config,
            Intent::Reconcile {
                model: !replace_model
                    && pending
                        .as_ref()
                        .is_none_or(|patch| patch.model_id == NullableSetting::Unchanged),
                thinking: !replace_thinking
                    && pending.as_ref().is_none_or(|patch| {
                        patch.thinking_option_id == NullableSetting::Unchanged
                            && !matches!(patch.model_id, NullableSetting::Set(_))
                    }),
            },
        )
        .await
    }

    async fn configure_open(
        &mut self,
        config: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        let Some(pending) = self.deferred.clone() else {
            return self.configure(config, Intent::Open).await;
        };
        let mut patch = pending.patch();
        let open = Plan::new(config, &self.config, &self.state, Intent::Open)?;
        // Resume reconciles native identity even when the durable handle still carries
        // a model command. Replaying an already selected model would reset its level.
        patch.model_id = if open.model.is_some() {
            config
                .model
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set)
        } else {
            NullableSetting::Unchanged
        };
        if open.thinking.is_some() || pending.config.thinking_option_id != config.thinking_option_id
        {
            patch.thinking_option_id = config
                .thinking_option_id
                .clone()
                .map_or(NullableSetting::Clear, NullableSetting::Set);
        }
        self.configure_deferred(config, &patch).await
    }

    async fn verify_session_identity(&mut self, state: &Value) -> Result<(), AgentSessionError> {
        if state["sessionId"] == self.id {
            return Ok(());
        }
        self.transport.diagnostics.observe(
            Diagnostic::new(Category::Protocol, "session_identity"),
            "get_state",
        );
        self.closed = true;
        self.transport.close().await?;
        Err(AgentSessionError::Failed)
    }

    async fn configure(
        &mut self,
        config: &StoredAgentConfig,
        intent: Intent<'_>,
    ) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        let plan = Plan::new(config, &self.config, &self.state, intent)?;
        // Match Pi's controls: a model switch owns its new default. Only an explicit
        // thinking change overrides it; never restore the previous model's level.
        let result = async {
            if let Some((provider, model_id)) = plan.model {
                self.request(
                    json!({"type":"set_model","provider":provider,"modelId":model_id}),
                    false,
                )
                .await?;
                if !matches!(intent, Intent::Reconcile { .. }) {
                    // A successful replacement owns the model and its default level,
                    // even if a following thinking command is rejected.
                    self.suppressed = SuppressedControls::default();
                    self.reset = ResetIntent::None;
                }
            }
            if let Some(level) = plan.thinking {
                let result = self.request(json!({"type":"set_thinking_level", "level":level}), false)
                    .await;
                if matches!(result, Err(AgentSessionError::Rejected))
                    && matches!(intent, Intent::Patch(patch) if patch.thinking_option_id == NullableSetting::Clear)
                {
                    self.reset = ResetIntent::None;
                    self.runtime_changed = true;
                }
                result?;
                self.suppressed.thinking = false;
                if config.thinking_option_id.is_some() {
                    self.reset = ResetIntent::None;
                }
            }
            Ok(())
        }
        .await;
        // A model may have changed even when the subsequent thinking command failed.
        // Retain that readback for observation rather than reporting stale runtime state.
        let readback = if plan.changes_native_state() {
            self.synchronize(false).await.map(|_| ())
        } else {
            Ok(())
        };
        result?;
        readback?;
        if !self.has_input
            && matches!(intent, Intent::Patch(patch) if patch.thinking_option_id == NullableSetting::Clear)
        {
            self.reset = ResetIntent::UntilInput;
        }
        if !matches!(intent, Intent::Reconcile { .. }) {
            self.suppressed = SuppressedControls::default();
        }
        self.config.clone_from(config);
        Ok(())
    }

    async fn configure_deferred(
        &mut self,
        config: &StoredAgentConfig,
        patch: &ConfigPatch,
    ) -> Result<(), AgentSessionError> {
        let result = self.configure(config, Intent::Patch(patch)).await;
        // A definitive rejection consumes this attempt, including any partial native
        // success. Retain pending intent only when a transport failure leaves it uncertain.
        if result.is_ok() || matches!(result, Err(AgentSessionError::Rejected)) {
            self.deferred = None;
            self.runtime_changed = true;
        }
        result
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
        if let Some(pending) = self.deferred.clone() {
            self.configure_deferred(&pending.config, &pending.patch())
                .await?;
        }
        if self.config != *config {
            self.configure(config, Intent::Turn).await?;
        }
        if !images.is_empty()
            && !self.state["model"]["input"]
                .as_array()
                .is_some_and(|types| types.iter().any(|kind| kind == "image"))
        {
            return Err(AgentSessionError::Rejected);
        }
        // This ordering barrier drains notifications from prior/independent native work.
        // Never let a stale settlement attach itself to the next host turn.
        if self.synchronize(false).await? {
            return Err(AgentSessionError::Rejected);
        }
        let turn = Uuid::new_v4().to_string();
        let submitted = Stream::submitted(&self.images, &turn, prompt, &text)?;
        self.transport.diagnostics.clear();
        self.stream.begin(turn.clone());
        self.stream.buffering = true;
        let input_position = self.stream.events.len();
        let result = self
            .transport
            .prompt(
                json!({"type":"prompt","message":text,"images":images}),
                |event| {
                    self.runtime_changed |= observe_thinking(&mut self.state, event);
                    self.stream.consume(event).map(|_| ()).map_err(|_| {
                        let diagnostic = self
                            .stream
                            .failure
                            .filter(|failure| failure.source == "display_limit")
                            .unwrap_or(Diagnostic::new(Category::Protocol, "prompt_event"));
                        self.stream.failure = Some(diagnostic);
                        diagnostic
                    })
                },
            )
            .await?;
        self.has_input |= self.stream.running || self.stream.settled;
        match result["disposition"].as_str() {
            Some("started") => {
                self.has_input = true;
                if !self.stream.settled {
                    self.active = Some(turn.clone());
                }
            }
            Some("handled") => {
                let busy = self.finish_handled_admission().await?;
                self.has_input |= self.stream.running || self.stream.settled;
                if busy || self.stream.running {
                    self.stream.resume_running();
                    self.has_input = true;
                    self.active = Some(turn.clone());
                } else if !self.stream.settled {
                    self.stream.handled()?;
                }
            }
            _ => {
                self.transport
                    .diagnostics
                    .observe(Diagnostic::new(Category::Protocol, "disposition"), "prompt");
                self.closed = true;
                self.transport.close().await?;
                return Err(AgentSessionError::Failed);
            }
        }
        for (index, event) in submitted.into_iter().enumerate() {
            self.stream.events.insert(input_position + index, event);
        }
        self.stream.end_buffering();
        self.report_failure();
        Ok(turn)
    }

    async fn finish_handled_admission(&mut self) -> Result<bool, AgentSessionError> {
        let result = tokio::time::timeout(self.transport.admission_deadline, async {
            let mut busy = self.synchronize(true).await?;
            while busy && !self.stream.running {
                let event = self.transport.receive().await?;
                self.runtime_changed |= observe_thinking(&mut self.state, &event);
                self.stream.consume(&event)?;
                if event["type"] == "compaction_end" || self.stream.settled {
                    busy = self.synchronize(true).await?;
                }
            }
            Ok(busy)
        })
        .await;
        match result {
            Ok(Ok(busy)) => Ok(busy),
            Ok(Err(error)) => {
                self.closed = true;
                let _ = self.transport.close().await;
                self.stream.fail();
                Err(error)
            }
            Err(_) => {
                self.transport.diagnostics.observe(
                    Diagnostic::new(Category::Timeout, "handled_settlement"),
                    "prompt",
                );
                self.closed = true;
                self.transport.close().await?;
                self.stream.fail();
                Err(AgentSessionError::Failed)
            }
        }
    }

    async fn request(
        &mut self,
        message: Value,
        admission: bool,
    ) -> Result<Value, AgentSessionError> {
        let state_request = message["type"] == "get_state";
        let mut thinking_observed = false;
        let was_buffering = self.stream.buffering;
        self.stream.buffering = true;
        let result = self
            .transport
            .request(message, |event| {
                thinking_observed |= thinking_level(event).is_some();
                self.runtime_changed |= observe_thinking(&mut self.state, event);
                consume_stream(&mut self.stream, &mut self.active, event, admission).map_err(|_| {
                    let diagnostic = self
                        .stream
                        .failure
                        .filter(|failure| failure.source == "display_limit")
                        .unwrap_or(Diagnostic::new(Category::Protocol, "control_event"));
                    self.stream.failure = Some(diagnostic);
                    diagnostic
                })
            })
            .await;
        if !was_buffering {
            self.stream.end_buffering();
        }
        self.report_failure();
        let mut response = result?;
        // A snapshot may predate notifications delivered during this request. Preserve
        // their latest valid level, even when it repeats the value already cached.
        if state_request
            && thinking_observed
            && let Some(state) = response.as_object_mut()
        {
            state.insert("thinkingLevel".into(), self.state["thinkingLevel"].clone());
        }
        Ok(response)
    }

    async fn synchronize(&mut self, admission: bool) -> Result<bool, AgentSessionError> {
        let state = self.request(json!({"type":"get_state"}), admission).await?;
        self.verify_session_identity(&state).await?;
        let busy = state["isStreaming"]
            .as_bool()
            .ok_or(AgentSessionError::Failed)?
            || state["isCompacting"]
                .as_bool()
                .ok_or(AgentSessionError::Failed)?
            || state["pendingMessageCount"].as_u64().unwrap_or_default() > 0;
        self.runtime_changed |= self.state["thinkingLevel"] != state["thinkingLevel"]
            || self.state["model"] != state["model"];
        self.state = state;
        Ok(busy)
    }

    fn report_failure(&self) {
        if self.stream.settled
            && let Some(diagnostic) = self.stream.failure
        {
            self.transport.diagnostics.observe(diagnostic, "settlement");
        }
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        self.runtime_changed |= observe_thinking(&mut self.state, event);
        consume_stream(&mut self.stream, &mut self.active, event, false)?;
        self.report_failure();
        Ok(())
    }

    fn info(&self) -> StoredAgentRuntimeInfo {
        let selection = Selection::read(&self.state);
        let extra = selection
            .thinking
            .as_ref()
            .map(|level| BTreeMap::from([("thinkingLevel".into(), json!(level))]));
        let mut info = StoredAgentRuntimeInfo {
            provider: PROVIDER.to_owned(),
            session_id: Some(self.id.clone()),
            model: selection.model,
            thinking_option_id: selection.thinking,
            mode_id: None,
            extra,
        };
        if self.deferred.is_some() {
            info.extra
                .get_or_insert_with(BTreeMap::new)
                .insert("configPending".into(), json!(true));
        }
        info
    }
}

fn consume_stream(
    stream: &mut Stream,
    active: &mut Option<String>,
    event: &Value,
    admission: bool,
) -> Result<(), AgentSessionError> {
    if admission || active.is_some() {
        if stream.consume(event)? && !admission {
            *active = None;
        }
    } else if matches!(
        event["type"].as_str(),
        Some("compaction_start" | "compaction_end")
    ) {
        stream.compact(event, false)?;
    }
    Ok(())
}

fn thinking_level(event: &Value) -> Option<&str> {
    if event["type"] != "thinking_level_changed" {
        return None;
    }
    // Native metadata may evolve independently of the host's selectable controls.
    event["level"]
        .as_str()
        .filter(|level| config::valid_thinking_level_id(level))
}

fn observe_thinking(state: &mut Value, event: &Value) -> bool {
    let Some(level) = thinking_level(event) else {
        return false;
    };
    if state["thinkingLevel"] == level {
        return false;
    }
    state["thinkingLevel"] = json!(level);
    true
}

impl AgentSession for Session {
    fn initialization_error(&self) -> Option<AgentSessionError> {
        self.initialization_error
    }

    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)
    }

    fn apply_config<'a>(
        &'a mut self,
        config: &'a StoredAgentConfig,
        patch: &'a ConfigPatch,
    ) -> AgentSessionFuture<'a, ConfigApplication> {
        Box::pin(async move {
            if self.closed {
                return Err(AgentSessionError::Rejected);
            }
            self.validate_config_update(config)?;
            if self.active.is_some() {
                self.deferred = Some(DeferredConfig::merge(
                    self.deferred.as_ref(),
                    &self.config,
                    config,
                    patch,
                ));
                self.runtime_changed = true;
                return Ok(ConfigApplication::Queued);
            }
            if self.deferred.is_some() {
                let pending =
                    DeferredConfig::merge(self.deferred.as_ref(), &self.config, config, patch);
                self.configure_deferred(&pending.config, &pending.patch())
                    .await?;
            } else {
                self.configure(config, Intent::Patch(patch)).await?;
            }
            // Returning the snapshot transfers publication to the manager, whose pending
            // runtime retains it if persistence fails. Failed controls keep the dirty flag.
            self.runtime_changed = false;
            Ok(ConfigApplication::Applied(self.info()))
        })
    }

    fn effective_config(&self) -> Option<StoredAgentConfig> {
        if let Some(pending) = &self.deferred {
            return Some(pending.config.clone());
        }
        let mut config = self.config.clone();
        let selection = Selection::read(&self.state);
        if !self.suppressed.model
            && let Some(model) = selection.model
        {
            config.model = Some(model);
        }
        // Readback confirms an explicit preference but must not create one. None lets
        // Pi's restored state remain authoritative on every subsequent resume.
        // Missing native metadata leaves an explicit preference intact until a valid
        // readback confirms it or the caller explicitly clears it.
        if !self.suppressed.thinking
            && config.thinking_option_id.is_some()
            && let Some(level) = selection.thinking
        {
            config.thinking_option_id = Some(level);
        }
        Some(config)
    }

    fn take_runtime_observation(&mut self) -> Option<StoredAgentRuntimeInfo> {
        std::mem::take(&mut self.runtime_changed).then(|| self.info())
    }

    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.stream
            .failure
            .filter(|diagnostic| diagnostic.category != Category::Unknown)
            .or_else(|| self.transport.diagnostics.failure())
            .or(self.stream.failure)
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        if self
            .failure_diagnostic()
            .is_some_and(|diagnostic| diagnostic.source != "display_limit")
        {
            if let Some(message) = self.stream.explanation.message() {
                return Some(Cow::Borrowed(message));
            }
            if let Some(message) = self.transport.diagnostics.message() {
                return Some(Cow::Owned(message));
            }
        }
        self.failure_diagnostic().map(|diagnostic| {
            Cow::Borrowed(if diagnostic.source == "display_limit" {
                "The provider output exceeded the display limit."
            } else {
                diagnostic.category.message()
            })
        })
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async {
            // Observing an admitted turn must not put it behind a control deadline.
            // Streaming notifications keep this snapshot current until idle readback is safe.
            // Rejected initialization already attempted readback. Expose its last verified
            // facts even if the transport subsequently exited, so consumption can persist.
            if self.active.is_none() && self.initialization_error.is_none() {
                self.synchronize(false).await?;
            }
            self.runtime_changed = false;
            Ok(self.info())
        })
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let mut handle = AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: self.state.get("sessionFile").cloned(),
            metadata: Some(BTreeMap::from([
                ("cwd".into(), json!(self.cwd)),
                ("nativeCwd".into(), json!(self.native_cwd)),
                ("hasInput".into(), json!(self.has_input)),
            ])),
        };
        // Setup-only setters have no durable native history. Carry a confirmed reset
        // through the existing deferred handle, without making it a saved preference
        // or reapplying it in this writer. Once input exists, Pi owns its persistence.
        let reset = if !self.has_input
            && self.reset == ResetIntent::UntilInput
            && self.deferred.is_none()
        {
            let config = self.effective_config()?;
            Some(DeferredConfig::merge(
                None,
                &config,
                &config,
                &ConfigPatch {
                    thinking_option_id: NullableSetting::Clear,
                    ..ConfigPatch::default()
                },
            ))
        } else {
            None
        };
        if let Some(pending) = self.deferred.as_ref().or(reset.as_ref()) {
            handle
                .metadata
                .as_mut()?
                .insert("deferredConfig".into(), serde_json::to_value(pending).ok()?);
        }
        Some(handle)
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

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(info) = self.take_runtime_observation() {
            return Ok(Some(AgentTurnEvent::RuntimeInfo(info)));
        }
        if let Some(event) = self.stream.pop() {
            return Ok(Some(event));
        }
        if self.closed {
            return Ok(None);
        }
        let result = self.transport.poll().and_then(|event| {
            if let Some(event) = event {
                self.consume(&event)?;
            }
            Ok(())
        });
        if result.is_err() {
            self.transport
                .diagnostics
                .observe(Diagnostic::new(Category::Protocol, "event"), "poll");
            self.closed = true;
            self.active = None;
            self.stream.fail();
        }
        if std::mem::take(&mut self.runtime_changed) {
            return Ok(Some(AgentTurnEvent::RuntimeInfo(self.info())));
        }
        Ok(self.stream.pop())
    }

    fn cancel_turn<'a>(&'a mut self, turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.closed || self.active.as_deref() != Some(turn_id) {
                return Err(AgentSessionError::Rejected);
            }
            self.request(json!({"type":"abort"}), false).await?;
            let result = tokio::time::timeout(ABORT_SETTLEMENT_TIMEOUT, async {
                while self.active.is_some() {
                    let event = self.transport.receive().await?;
                    self.consume(&event)?;
                }
                Ok(())
            })
            .await
            .unwrap_or(Err(AgentSessionError::Failed));
            if result.is_err() {
                self.transport.diagnostics.observe(
                    Diagnostic::new(Category::Timeout, "abort_settlement"),
                    "abort",
                );
                self.closed = true;
                self.active = None;
                self.transport.close().await?;
                self.stream.fail();
            }
            result
        })
    }

    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            self.closed = true;
            self.transport.close().await
        })
    }
}

#[cfg(test)]
mod tests;
