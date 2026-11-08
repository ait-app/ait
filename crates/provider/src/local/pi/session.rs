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
    streaming::Stream,
    transport::{self, Transport},
};
use crate::local::prompt_blocks;
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::protocol::prompt::AgentPrompt;

const ABORT_SETTLEMENT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STATE_READS: usize = 3;

#[cfg(all(test, unix))]
mod tests;

#[derive(Clone, Copy)]
enum Synchronization {
    Opening,
    Prompt,
    Handled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigurationState {
    Applied,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistenceState {
    Empty,
    Observed,
    Required,
}

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
    configuration_state: ConfigurationState,
    rejected_thinking: Option<String>,
    images: crate::local::images::ImageStore,
    active: Option<String>,
    stream: Stream,
    persistence_state: PersistenceState,
    closed: bool,
}

/// Open `spec` as a fresh session or resume the verified `handle`.
/// Returns rejection for foreign/missing history and failure for native protocol errors.
pub(super) async fn open(
    client: &PiClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::spec(spec)?;
    let id = handle.map_or_else(
        || Uuid::new_v4().to_string(),
        |handle| handle.session_id.clone(),
    );
    let metadata = handle.and_then(|handle| handle.metadata.as_ref());
    let mut persistence_state = if metadata
        .is_some_and(|data| data.get("hasInput") == Some(&Value::Bool(true)))
    {
        PersistenceState::Required
    } else if metadata.is_some_and(|data| data.get("observedWork") == Some(&Value::Bool(true))) {
        PersistenceState::Observed
    } else {
        PersistenceState::Empty
    };
    let native_cwd = metadata
        .and_then(|data| data.get("nativeCwd")?.as_str())
        .unwrap_or(&spec.cwd);
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
        let path = handle
            .native_handle
            .as_ref()
            .and_then(Value::as_str)
            .or_else(|| handle.metadata.as_ref()?.get("sessionFile")?.as_str());
        // The manager may have saved this handle before Pi finished its shutdown write.
        if persistence_state == PersistenceState::Observed
            && saved_handle_matches(client, handle, native_cwd).await
        {
            persistence_state = PersistenceState::Required;
        }
        if persistence_state == PersistenceState::Required {
            let path = path.ok_or(AgentSessionError::Rejected)?;
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
    transport::version(client).await?;
    let mut session = Session {
        transport: Transport::spawn(client, &spec.cwd, &args, spec.confinement.as_ref())?,
        id,
        cwd: spec.cwd.clone(),
        native_cwd: native_cwd.to_owned(),
        state: Value::Null,
        runtime_changed: false,
        config: StoredAgentConfig::default(),
        configuration_state: ConfigurationState::Applied,
        rejected_thinking: None,
        images: client.images.clone(),
        active: None,
        stream: Stream::default(),
        persistence_state,
        closed: false,
    };
    if session.synchronize(Synchronization::Opening).await? {
        return Err(AgentSessionError::Failed);
    }
    session.runtime_changed = false;
    match session
        .configure(&spec.config, Synchronization::Opening)
        .await
    {
        Err(AgentSessionError::Rejected)
            if session
                .transport
                .diagnostics
                .failure()
                .is_some_and(|diagnostic| diagnostic.source == "thinking_configuration") =>
        {
            // Keep the writer and its explanation until prompt admission can publish the
            // normal failure notice. Opening errors otherwise discard session diagnostics.
        }
        result => {
            result?;
        }
    }
    Ok(session)
}

impl Session {
    async fn verify_session_identity(&mut self, state: &Value) -> Result<(), AgentSessionError> {
        if state["sessionId"] == self.id {
            return Ok(());
        }
        self.transport.diagnostics.observe(
            Diagnostic::new(Category::Protocol, "session_identity"),
            "get_state",
        );
        self.closed = true;
        self.close_transport().await?;
        Err(AgentSessionError::Failed)
    }

    async fn configure(
        &mut self,
        config: &StoredAgentConfig,
        synchronization: Synchronization,
    ) -> Result<bool, AgentSessionError> {
        config::validate(config)?;
        let pending = self.configuration_state == ConfigurationState::Pending;
        let model_changed = config.model.as_deref().is_some_and(|model| {
            if pending {
                config::split_model(model) != config::model_identity(&self.state["model"])
            } else {
                self.config.model != config.model
            }
        });
        let thinking_changed = self.config.thinking_option_id != config.thinking_option_id
            || self.rejected_thinking.is_some();
        // set_model resets Pi's thinking to the destination model's saved default.
        // Prefer the requested level so a previous clamp does not become permanent.
        // Without an override, preserve the current native level, including extension changes.
        let restoration_seed = if model_changed && !thinking_changed {
            config
                .thinking_option_id
                .as_deref()
                .or_else(|| self.state["thinkingLevel"].as_str())
                .map(str::to_owned)
        } else {
            None
        };
        // A rejected second control can leave the model changed. Pending model comparisons
        // use native identity so reverting restores it without resending an unchanged model.
        self.configuration_state = ConfigurationState::Pending;
        if model_changed && let Some(model) = &config.model {
            let (provider, model_id) =
                config::split_model(model).ok_or(AgentSessionError::Rejected)?;
            let model = self
                .transport
                .request(json!({"type":"set_model","provider":provider,"modelId":model_id}))
                .await?;
            self.runtime_changed |=
                config::model_identity(&self.state["model"]) != config::model_identity(&model);
            self.state["model"] = model;
            // The model response does not include its reset thinking level. Never publish
            // the previous model's level if the following control or readback fails.
            self.runtime_changed |= !self.state["thinkingLevel"].is_null();
            self.state["thinkingLevel"] = Value::Null;
        }
        // Restore the preferred level against the destination capabilities. Pi has no unset
        // RPC, so clearing an override tries medium as a best-effort seed (ADR-156).
        let thinking_seed = if thinking_changed {
            config.thinking_option_id.is_none().then_some("medium")
        } else {
            restoration_seed.as_deref()
        };
        // Opening preferences can come from another model's saved or imported history.
        // Clamp against native capabilities while retaining the original requested seed.
        let fallback_thinking = if matches!(synchronization, Synchronization::Opening) {
            config.thinking_option_id.as_deref().or(thinking_seed)
        } else {
            thinking_seed
        }
        .and_then(|level| config::clamp_thinking_level(&self.state["model"], level));
        let thinking = if thinking_changed && !matches!(synchronization, Synchronization::Opening) {
            config
                .thinking_option_id
                .as_deref()
                .or(fallback_thinking.as_deref())
        } else {
            fallback_thinking.as_deref()
        };
        if let Some(level) = thinking {
            let result = self
                .transport
                .request(json!({"type":"set_thinking_level","level":level}))
                .await;
            match result {
                Ok(_) => {}
                Err(AgentSessionError::Rejected) => {
                    if config.thinking_option_id.is_some() {
                        self.rejected_thinking
                            .clone_from(&config.thinking_option_id);
                        self.transport.diagnostics.clear();
                        // Ambiguous thinking preserves the known control rejection, but a
                        // broken readback must retain its diagnostic and retire the writer.
                        if let Err(error) = self.synchronize(synchronization).await
                            && error != AgentSessionError::Rejected
                        {
                            self.closed = true;
                            let _ = self.close_transport().await;
                            return Err(error);
                        }
                        // This definitive control rejection has a known explanation. Native
                        // wording must not give its guessed category precedence over it.
                        self.transport.diagnostics.clear();
                        self.transport.diagnostics.observe_failure(
                            Diagnostic::new(Category::Unknown, "thinking_configuration"),
                            "Pi rejected the thinking level. Choose a supported level before retrying. The prompt was not submitted.",
                            "set_thinking_level",
                        );
                        return Err(AgentSessionError::Rejected);
                    }
                    // With no override the seed is ours. Release the override even if Pi
                    // refuses that seed, then read back and retain its actual native level.
                    self.transport.diagnostics.clear();
                }
                Err(error) => return Err(error),
            }
        }
        let busy = if model_changed || thinking.is_some() {
            self.synchronize(synchronization).await?
        } else {
            false
        };
        self.rejected_thinking = None;
        self.config.clone_from(config);
        self.configuration_state = ConfigurationState::Applied;
        Ok(busy)
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
        let (text, images) = prompt_blocks::split(prompt.blocks()?);
        if self
            .rejected_thinking
            .as_deref()
            .is_some_and(|level| config.thinking_option_id.as_deref() == Some(level))
        {
            return Err(AgentSessionError::Rejected);
        }
        self.transport.diagnostics.clear();
        // Check native work before any control mutation, including extension-owned turns.
        if self.synchronize(Synchronization::Prompt).await? {
            return Err(AgentSessionError::Busy);
        }
        let configured =
            self.config != *config || self.configuration_state == ConfigurationState::Pending;
        // Configuration's final readback also drains prior native notifications and checks
        // for work started during the controls, serving as the prompt ordering barrier.
        if configured && self.configure(config, Synchronization::Prompt).await? {
            return Err(AgentSessionError::Busy);
        }
        if !images.is_empty()
            && !self.state["model"]["input"]
                .as_array()
                .is_some_and(|types| types.iter().any(|kind| kind == "image"))
        {
            return Err(AgentSessionError::Rejected);
        }
        let turn = Uuid::new_v4().to_string();
        let submitted = Stream::submitted(&self.images, &turn, prompt, &text)?;
        self.transport.diagnostics.clear();
        self.stream.begin(turn.clone());
        self.stream.admission = true;
        let result = self
            .admit(
                &turn,
                json!({"type":"prompt","message":text,"images":images}),
                submitted,
            )
            .await;
        if result.is_err() && (self.closed || matches!(result, Err(AgentSessionError::Failed))) {
            self.closed = true;
            self.active = None;
            let _ = self.close_transport().await;
            self.stream.fail();
        } else {
            // Check once after admission has consumed all native work and state readbacks.
            self.observe_persisted_work().await;
        }
        self.stream.end_admission();
        result?;
        self.report_failure();
        Ok(turn)
    }

    async fn admit(
        &mut self,
        turn: &str,
        request: Value,
        submitted: Vec<AgentTurnEvent>,
    ) -> Result<(), AgentSessionError> {
        let result = self
            .transport
            .prompt(request, |event| {
                observe_thinking(&mut self.state, &mut self.runtime_changed, event);
                self.stream.consume(event).map(|_| ()).map_err(|_| {
                    let diagnostic = self
                        .stream
                        .failure
                        .filter(|failure| failure.source == "display_limit")
                        .unwrap_or(Diagnostic::new(Category::Protocol, "prompt_event"));
                    self.stream.failure = Some(diagnostic);
                    diagnostic
                })
            })
            .await;
        // A definitive rejection takes precedence over unrelated extension activity.
        let observed_work = !matches!(result, Err(AgentSessionError::Rejected))
            && (self.stream.running || self.stream.settled);
        if result
            .as_ref()
            .is_ok_and(|response| response["disposition"] == "started")
        {
            self.persistence_state = PersistenceState::Required;
        } else if observed_work {
            self.observe_native_work();
        }
        // Uncertain admission retains the prompt ahead of observed work and cleanup.
        if result.is_ok() || observed_work {
            let failure = self.stream.failure;
            let projection = self.stream.insert_submitted(submitted);
            if result.is_ok() {
                projection?;
            } else {
                // Input retention is best effort after admission has already failed.
                self.stream.failure = failure;
            }
        }
        let result = result?;
        match result["disposition"].as_str() {
            Some("started") => {
                if !self.stream.settled {
                    self.active = Some(turn.to_owned());
                }
            }
            Some("handled") => {
                let result = self.finish_handled_admission().await;
                if self.stream.running || self.stream.settled {
                    self.observe_native_work();
                }
                let busy = result?;
                if busy || self.stream.running {
                    self.stream.resume_running();
                    self.observe_native_work();
                    self.active = Some(turn.to_owned());
                } else if !self.stream.settled {
                    self.stream.handled()?;
                }
            }
            _ => {
                self.transport
                    .diagnostics
                    .observe(Diagnostic::new(Category::Protocol, "disposition"), "prompt");
                self.closed = true;
                self.close_transport().await?;
                return Err(AgentSessionError::Failed);
            }
        }
        Ok(())
    }

    fn observe_native_work(&mut self) {
        if self.persistence_state == PersistenceState::Empty {
            self.persistence_state = PersistenceState::Observed;
        }
    }

    async fn observe_persisted_work(&mut self) {
        if self.persistence_state != PersistenceState::Observed {
            return;
        }
        let Some(path) = self.state["sessionFile"].as_str() else {
            return;
        };
        // Pi assigns sessionFile before flushing history; verify the saved identity.
        if saved_session_matches(path, &self.id, &self.native_cwd).await {
            self.persistence_state = PersistenceState::Required;
        }
    }

    async fn close_transport(&mut self) -> Result<(), AgentSessionError> {
        let result = self.transport.close().await;
        // A negative check while Pi was alive may have raced its final history write.
        self.observe_persisted_work().await;
        result
    }

    async fn finish_handled_admission(&mut self) -> Result<bool, AgentSessionError> {
        let result = tokio::time::timeout(self.transport.admission_deadline, async {
            let mut busy = self.synchronize(Synchronization::Handled).await?;
            while busy && !self.stream.running {
                let event = self.transport.receive().await?;
                observe_thinking(&mut self.state, &mut self.runtime_changed, &event);
                self.stream.consume(&event)?;
                if event["type"] == "compaction_end" || self.stream.settled {
                    busy = self.synchronize(Synchronization::Handled).await?;
                }
            }
            Ok(busy)
        })
        .await;
        match result {
            Ok(Ok(busy)) => Ok(busy),
            Ok(Err(error)) => {
                self.closed = true;
                let _ = self.close_transport().await;
                Err(error)
            }
            Err(_) => {
                self.transport.diagnostics.observe(
                    Diagnostic::new(Category::Timeout, "handled_settlement"),
                    "prompt",
                );
                self.closed = true;
                self.close_transport().await?;
                Err(AgentSessionError::Failed)
            }
        }
    }

    async fn synchronize(
        &mut self,
        synchronization: Synchronization,
    ) -> Result<bool, AgentSessionError> {
        let mut busy = false;
        let admission = matches!(synchronization, Synchronization::Handled);
        // Completed controls precede this barrier and are superseded by its snapshots.
        self.consume_pending(admission, None)?;
        let mut observed_level = None;
        for _ in 0..MAX_STATE_READS {
            let thinking_conflict;
            (busy, thinking_conflict) = self
                .synchronize_snapshot(admission, &mut observed_level)
                .await?;
            if !thinking_conflict {
                return Ok(busy);
            }
        }
        // Only prompt admission needs stable thinking state. Opening and already accepted
        // work can report the latest verified snapshot while extensions keep changing it.
        if matches!(synchronization, Synchronization::Prompt) {
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Protocol, "thinking_conflict"),
                "Pi's thinking level kept changing while preparing the prompt. The prompt was not submitted; retry after native settings settle.",
                "get_state",
            );
            Err(AgentSessionError::Rejected)
        } else {
            Ok(busy)
        }
    }

    async fn synchronize_snapshot(
        &mut self,
        admission: bool,
        observed_level: &mut Option<String>,
    ) -> Result<(bool, bool), AgentSessionError> {
        let mut state = self.transport.request(json!({"type":"get_state"})).await?;
        self.verify_session_identity(&state).await?;
        if !state["thinkingLevel"]
            .as_str()
            .is_some_and(config::valid_thinking_level_id)
        {
            state["thinkingLevel"] = Value::Null;
        }
        let streaming = state["isStreaming"]
            .as_bool()
            .ok_or(AgentSessionError::Failed)?;
        let compacting = state["isCompacting"]
            .as_bool()
            .ok_or(AgentSessionError::Failed)?;
        self.consume_pending(admission, Some(observed_level))?;
        // Retain the latest mid-read notification until a snapshot agrees, even if Pi
        // announces it only once while its state reads continue to lag.
        let thinking_conflict = state["thinkingLevel"]
            .as_str()
            .zip(observed_level.as_deref())
            .is_some_and(|(snapshot, observed)| snapshot != observed);
        self.runtime_changed |= self.state["thinkingLevel"] != state["thinkingLevel"]
            || config::model_identity(&self.state["model"])
                != config::model_identity(&state["model"]);
        self.state = state;
        Ok((streaming || compacting, thinking_conflict))
    }

    fn consume_pending(
        &mut self,
        admission: bool,
        mut observed_level: Option<&mut Option<String>>,
    ) -> Result<(), AgentSessionError> {
        let pending: Vec<_> = self.transport.drain_pending().collect();
        for event in pending {
            if let Some(observed_level) = observed_level.as_deref_mut()
                && let Some(level) = thinking_notification(&event)
            {
                *observed_level = Some(level.to_owned());
            }
            if admission {
                self.stream.consume(&event)?;
            } else {
                self.consume_stream(&event)?;
            }
        }
        Ok(())
    }

    fn report_failure(&self) {
        if self.stream.settled
            && let Some(diagnostic) = self.stream.failure
        {
            self.transport.diagnostics.observe(diagnostic, "settlement");
        }
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        observe_thinking(&mut self.state, &mut self.runtime_changed, event);
        self.consume_stream(event)
    }

    fn consume_stream(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if self.active.is_none() {
            if matches!(
                event["type"].as_str(),
                Some("compaction_start" | "compaction_end")
            ) {
                self.stream.compact(event, false)?;
            }
            return Ok(());
        }
        if self.stream.consume(event)? {
            self.active = None;
        }
        self.report_failure();
        Ok(())
    }

    fn info(&self) -> StoredAgentRuntimeInfo {
        StoredAgentRuntimeInfo {
            provider: PROVIDER.to_owned(),
            session_id: Some(self.id.clone()),
            model: config::model_identity(&self.state["model"])
                .map(|(provider, id)| format!("{provider}/{id}")),
            thinking_option_id: self.state["thinkingLevel"].as_str().map(str::to_owned),
            mode_id: None,
            extra: None,
        }
    }
}

async fn saved_handle_matches(
    client: &PiClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> bool {
    let client = client.clone();
    let handle = handle.clone();
    let cwd = cwd.to_owned();
    tokio::task::spawn_blocking(move || {
        super::history::matches_saved_session_header(&client, &handle, &cwd)
    })
    .await
    .unwrap_or(false)
}

async fn saved_session_matches(path: &str, id: &str, cwd: &str) -> bool {
    let path = Path::new(path).to_owned();
    let id = id.to_owned();
    let cwd = cwd.to_owned();
    tokio::task::spawn_blocking(move || super::history::matches_session_header(&path, &id, &cwd))
        .await
        .unwrap_or(false)
}

fn observe_thinking(state: &mut Value, changed: &mut bool, event: &Value) {
    let Some(level) = thinking_notification(event) else {
        return;
    };
    if state["thinkingLevel"] == level {
        return;
    }
    state["thinkingLevel"] = json!(level);
    *changed = true;
}

fn thinking_notification(event: &Value) -> Option<&str> {
    if event["type"] != "thinking_level_changed" {
        return None;
    }
    // Native metadata may evolve independently of the host's selectable controls.
    event["level"]
        .as_str()
        .filter(|level| config::valid_thinking_level_id(level))
}

#[async_trait::async_trait]
impl AgentSession for Session {
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

    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(self.info())
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let mut metadata = BTreeMap::from([
            ("cwd".into(), json!(self.cwd)),
            ("nativeCwd".into(), json!(self.native_cwd)),
            (
                "hasInput".into(),
                json!(self.persistence_state == PersistenceState::Required),
            ),
        ]);
        if self.persistence_state == PersistenceState::Observed {
            metadata.insert("observedWork".into(), json!(true));
        }
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: self.state.get("sessionFile").cloned(),
            metadata: Some(metadata),
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

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if std::mem::take(&mut self.runtime_changed) {
            return Ok(Some(AgentTurnEvent::RuntimeInfo(self.info())));
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

    async fn cancel_turn(&mut self, turn_id: &str) -> Result<(), AgentSessionError> {
        if self.closed || self.active.as_deref() != Some(turn_id) {
            return Err(AgentSessionError::Rejected);
        }
        self.transport.request(json!({"type":"abort"})).await?;
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
            self.close_transport().await?;
            self.stream.fail();
        }
        result
    }

    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.closed = true;
        self.close_transport().await
    }
}
