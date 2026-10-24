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
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::diagnostics::{Category, Diagnostic};
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
    config: StoredAgentConfig,
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
    transport::version(client).await?;
    let mut session = Session {
        transport: Transport::spawn(client, &spec.cwd, &args)?,
        id,
        cwd: spec.cwd.clone(),
        native_cwd: handle
            .and_then(|handle| handle.metadata.as_ref()?.get("nativeCwd")?.as_str())
            .unwrap_or(&spec.cwd)
            .to_owned(),
        state: Value::Null,
        config: spec.config.clone(),
        images: client.images.clone(),
        active: None,
        stream: Stream::default(),
        has_input,
        closed: false,
    };
    let state = session
        .transport
        .request(json!({"type":"get_state"}))
        .await?;
    session.verify_session_identity(&state).await?;
    if state["isStreaming"] != false || state["isCompacting"] != false {
        return Err(AgentSessionError::Failed);
    }
    session.state = state;
    session.configure(&spec.config).await?;
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
        self.transport.close().await?;
        Err(AgentSessionError::Failed)
    }

    async fn configure(&mut self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        let Some(model) = &config.model else {
            self.config.clone_from(config);
            return Ok(());
        };
        let (provider, model_id) = config::split_model(model).ok_or(AgentSessionError::Rejected)?;
        self.transport
            .request(json!({"type":"set_model","provider":provider,"modelId":model_id}))
            .await?;
        let state = self.transport.request(json!({"type":"get_state"})).await?;
        self.verify_session_identity(&state).await?;
        self.state = state;
        self.config.clone_from(config);
        Ok(())
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
        if self.config != *config {
            self.configure(config).await?;
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
        self.stream.admission = true;
        let input_position = self.stream.events.len();
        let result = self
            .transport
            .prompt(
                json!({"type":"prompt","message":text,"images":images}),
                |event| {
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
        self.stream.end_admission();
        self.report_failure();
        Ok(turn)
    }

    async fn finish_handled_admission(&mut self) -> Result<bool, AgentSessionError> {
        let result = tokio::time::timeout(self.transport.admission_deadline, async {
            let mut busy = self.synchronize(true).await?;
            while busy && !self.stream.running {
                let event = self.transport.receive().await?;
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

    async fn synchronize(&mut self, admission: bool) -> Result<bool, AgentSessionError> {
        let state = self.transport.request(json!({"type":"get_state"})).await?;
        self.verify_session_identity(&state).await?;
        let busy = state["isStreaming"]
            .as_bool()
            .ok_or(AgentSessionError::Failed)?
            || state["isCompacting"]
                .as_bool()
                .ok_or(AgentSessionError::Failed)?
            || state["pendingMessageCount"].as_u64().unwrap_or_default() > 0;
        let pending: Vec<_> = self.transport.drain_pending().collect();
        for event in pending {
            if admission {
                self.stream.consume(&event)?;
            } else {
                self.consume(&event)?;
            }
        }
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
}

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

    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async {
            let model = &self.state["model"];
            Ok(StoredAgentRuntimeInfo {
                provider: PROVIDER.to_owned(),
                session_id: Some(self.id.clone()),
                model: model["provider"]
                    .as_str()
                    .zip(model["id"].as_str())
                    .map(|(provider, id)| format!("{provider}/{id}")),
                thinking_option_id: None,
                mode_id: None,
                extra: None,
            })
        })
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: self.state.get("sessionFile").cloned(),
            metadata: Some(BTreeMap::from([
                ("cwd".into(), json!(self.cwd)),
                ("nativeCwd".into(), json!(self.native_cwd)),
                ("hasInput".into(), json!(self.has_input)),
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

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
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
        Ok(self.stream.pop())
    }

    fn cancel_turn<'a>(&'a mut self, turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
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
