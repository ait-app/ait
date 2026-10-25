//! OMP's required ACP execution channel, including native feature and event extensions.

mod config;

use std::borrow::Cow;
use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

use super::{OmpClient, config as native_config};
use crate::local::{
    acp_interactions, acp_permissions,
    acp_stream::Stream as StandardStream,
    acp_transport::{self, Transport},
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::protocol::prompt::AgentPrompt;

#[derive(Debug)]
pub(super) struct Session {
    client: OmpClient,
    transport: Transport,
    id: String,
    cwd: String,
    initial: StoredAgentConfig,
    options: Value,
    capabilities: Value,
    extensions: Value,
    settled: bool,
    active: Option<(String, String)>,
    stream: super::stream::Stream,
    standard: StandardStream,
    children: super::subagents::Children,
    pending: BTreeMap<String, acp_permissions::Pending>,
    closed: bool,
    submitted: bool,
    created_at: String,
    terminal: Option<AgentTurnEvent>,
    history: Option<
        oneshot::Receiver<Result<crate::ports::native_history::SessionHistory, AgentSessionError>>,
    >,
    history_task: Option<AbortOnDropHandle<()>>,
}

/// Open one ACP connection and create or resume its native session without sending input.
/// Invalid handles/configuration reject before execution; native failures are propagated.
pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
    ephemeral: bool,
) -> Result<Session, AgentSessionError> {
    native_config::validate_spec(spec)?;
    let previous = if let Some(handle) = handle {
        Some(super::history::load(client, handle, &spec.cwd).await?)
    } else {
        None
    };
    let resuming = previous
        .as_ref()
        .is_some_and(|journal| journal.path.is_file());
    let previous_history = previous
        .as_ref()
        .zip(handle)
        .map(|(journal, handle)| journal.project(client, handle))
        .transpose()?;
    let (mut transport, initialized) = connect(client, spec, ephemeral).await?;
    let capabilities = initialized["agentCapabilities"].clone();
    let mut params = json!({"cwd":spec.cwd,"mcpServers":[]});
    let method = if resuming {
        if !capabilities["sessionCapabilities"]["resume"].is_object() {
            return Err(AgentSessionError::Rejected);
        }
        params["sessionId"] = json!(handle.expect("resuming requires a handle").session_id);
        "session/resume"
    } else {
        "session/new"
    };
    let result = transport.request(method, params).await?;
    let id = if resuming {
        handle
            .expect("resuming requires a handle")
            .session_id
            .clone()
    } else {
        native_config::text(&result, "sessionId")?.to_owned()
    };
    let mut options = crate::local::acp_config::state_with_limit(&result, 8192)?;
    config::apply(&mut transport, &id, &mut options, &spec.config, ephemeral).await?;
    let mut stream = previous_history.map_or_else(
        || super::stream::Stream::new(id.clone(), client.images.clone()),
        |(_, stream)| stream,
    );
    stream = stream.with_blobs(super::history::root(client)?.with_file_name("blobs"));
    stream.set_context(&config::context(&options));
    Ok(Session {
        client: client.clone(),
        transport,
        id,
        cwd: spec.cwd.clone(),
        initial: spec.config.clone(),
        options,
        capabilities,
        extensions: initialized["_meta"].clone(),
        settled: false,
        active: None,
        stream,
        standard: StandardStream::new(client.images.clone()),
        children: super::subagents::Children::default()
            .with_blobs(super::history::root(client)?.with_file_name("blobs")),
        pending: BTreeMap::new(),
        closed: false,
        submitted: resuming,
        created_at: previous
            .as_ref()
            .and_then(|journal| journal.header["timestamp"].as_str())
            .map_or_else(|| chrono::Utc::now().to_rfc3339(), str::to_owned),
        terminal: None,
        history: None,
        history_task: None,
    })
}

async fn connect(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    ephemeral: bool,
) -> Result<(Transport, Value), AgentSessionError> {
    let mut command = tokio::process::Command::new(&client.program);
    command
        .args(["--mode", "acp", "--no-pty"])
        .envs(client.environment.entries());
    if ephemeral {
        command.arg("--no-session");
    }
    if let Some(prompt) = &spec.config.system_prompt {
        command.args(["--append-system-prompt", prompt]);
    }
    if let Some(model) = &spec.config.model {
        let [provider, model] = native_config::model_route(model)?;
        command.args(["--provider", &provider, "--model", &model]);
    }
    let mut transport = Transport::spawn_with_output_limit(
        command,
        &spec.cwd,
        client.deadline,
        "Oh My Pi",
        64 * 1024 * 1024,
    )?;
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{"elicitation":{"form":{}},"_meta":{"sessionSettled":true,"omp":1,"ompNativeMcp":!ephemeral}},
        "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1
        || initialized["_meta"]["sessionSettled"]["version"] != 1
        || initialized["_meta"]["omp"]["version"] != 1
    {
        transport.diagnostics.observe_failure(
            Diagnostic::new(Category::Protocol, "omp_acp_capabilities"),
            "OMP ACP requires the session-settled and native-feature extensions (version 1). Install the supported OMP build described in docs/operations/oh-my-pi.md.", "initialize");
        return Err(AgentSessionError::Rejected);
    }
    Ok((transport, initialized))
}

impl Session {
    /// Read native bootstrap commands without creating input or a persisted conversation.
    /// Missing or malformed discovery notifications fail within the control deadline.
    pub(super) async fn commands(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        tokio::time::timeout(self.transport.control_deadline(), async {
            loop {
                let message = self.transport.receive().await?;
                let update = &message["params"]["update"];
                if message["method"] == "session/update"
                    && message["params"]["sessionId"] == self.id
                    && update["sessionUpdate"] == "available_commands_update"
                {
                    return native_config::commands(
                        &json!({"commands":update["availableCommands"]}),
                    );
                }
                self.consume(&message)?;
            }
        })
        .await
        .map_err(|_| AgentSessionError::Failed)?
    }

    /// Describe the selected model's ACP-advertised controls for a draft session.
    pub(super) fn draft_features(&self, requested: &StoredAgentConfig) -> Vec<Value> {
        config::features(&self.options, requested)
    }

    /// Read ACP model, mode and feature metadata without running inference.
    pub(super) fn details(&self) -> Result<crate::protocol::provider::Details, AgentSessionError> {
        config::details(&self.options)
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        self.validate_config_update(config)?;
        if self.closed
            || self.active.is_some()
            || !self.pending.is_empty()
            || prompt.output_schema.is_some()
            || (!prompt.images.is_empty()
                && self.capabilities["promptCapabilities"]["image"] != true)
        {
            return Err(AgentSessionError::Rejected);
        }
        if self.initial.system_prompt != config.system_prompt {
            let handle = self.persistence().ok_or(AgentSessionError::Failed)?;
            let client = self.client.clone();
            let spec = AgentSessionSpec {
                provider: "omp".to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
            };
            self.close().await?;
            *self = open(&client, &spec, Some(&handle), false).await?;
        }
        self.transport.diagnostics.clear();
        config::apply(
            &mut self.transport,
            &self.id,
            &mut self.options,
            config,
            false,
        )
        .await?;
        self.stream.set_context(&config::context(&self.options));
        let blocks = prompt.blocks()?;
        let turn = Uuid::new_v4().to_string();
        let rpc = self
            .transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"prompt":blocks,"_meta":{"turnId":turn}}),
            )
            .await?;
        self.stream.begin(turn.clone());
        self.standard.begin(turn.clone());
        self.settled = false;
        self.stream
            .pending_inputs
            .push_back(prompt.client_message_id.clone());
        self.active = Some((turn.clone(), rpc));
        self.initial.clone_from(config);
        Ok(turn)
    }

    fn consume(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        match message["method"].as_str() {
            Some("_omp/event") => self.consume_native(message)?,
            Some("_session/settled") => {
                if message["params"]["sessionId"] != self.id {
                    return Err(AgentSessionError::Failed);
                }
                if self
                    .active
                    .as_ref()
                    .is_some_and(|(turn, _)| message["params"]["turnId"] == *turn)
                {
                    self.settled = true;
                }
            }
            Some("session/update") => {
                if message["params"]["sessionId"] != self.id {
                    return Err(AgentSessionError::Failed);
                }
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "config_option_update" {
                    self.options = crate::local::acp_config::state_with_limit(update, 8192)?;
                    self.stream.set_context(&config::context(&self.options));
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                            &self.id,
                            &self.options,
                        )));
                } else if self.active.is_some() && update["_meta"]["providerError"] != true {
                    // The final response carries typed native errors; project those only
                    // through the bounded, redacted failure path rather than answer text.
                    // Message/tool updates are already projected from lossless native events.
                    // Standard compaction/context notifications retain their ACP semantics.
                    if matches!(
                        update["sessionUpdate"].as_str(),
                        Some("compaction_update" | "usage_update")
                    ) {
                        self.standard.update(update)?;
                        self.stream.events.extend(self.standard.events.drain(..));
                    }
                }
            }
            Some("session/request_permission" | "elicitation/create") => {
                if self.active.is_none()
                    || self.pending.len() >= 128
                    || self
                        .pending
                        .values()
                        .any(|pending| pending.rpc_id == message["id"])
                    || (message["params"].get("sessionId").is_some()
                        && message["params"]["sessionId"] != self.id)
                {
                    return Err(AgentSessionError::Failed);
                }
                let pending = if message["method"] == "session/request_permission" {
                    acp_permissions::capture(message, "omp")?
                } else {
                    acp_interactions::form(message, "omp")?
                };
                let id = native_config::text(&pending.request, "id")?.to_owned();
                self.stream
                    .events
                    .push_back(AgentTurnEvent::PermissionRequested(pending.request.clone()));
                self.pending.insert(id, pending);
            }
            Some(_) if message.get("id").is_some() => return Err(AgentSessionError::Failed),
            Some(_) => {}
            None => self.consume_response(message)?,
        }
        self.reconcile_if_settled();
        Ok(())
    }

    fn consume_native(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if message["params"]["sessionId"] != self.id {
            return Err(AgentSessionError::Failed);
        }
        let event = &message["params"]["event"];
        self.submitted |= event["type"] == "message_end" && event["message"]["role"] == "user";
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
        if matches!(
            event["type"].as_str(),
            Some("subagent_lifecycle" | "subagent_progress" | "subagent_event")
        ) {
            self.stream.events.extend(self.children.consume(
                event,
                &self.id,
                &self.cwd,
                &self.client.images,
            )?);
        } else if self.active.is_some() {
            self.stream.consume(event).map_err(|error| {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "omp_event"),
                    &error.to_string(),
                    "project",
                );
                AgentSessionError::Failed
            })?;
        }
        Ok(())
    }

    fn consume_response(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let (_, rpc) = self.active.as_ref().ok_or(AgentSessionError::Failed)?;
        if message["id"] != *rpc {
            return Err(AgentSessionError::Failed);
        }
        if self.terminal.is_some() {
            return Err(AgentSessionError::Failed);
        }
        let terminal = if let Some(error) = message.get("error") {
            self.transport
                .diagnostics
                .observe_native(error, "rpc", "session/prompt");
            AgentTurnEvent::Failed
        } else if let Some(error) = message["result"]["_meta"]
            .get("error")
            .filter(|error| !error.is_null())
        {
            self.transport
                .diagnostics
                .observe_native(error, "rpc", "session/prompt");
            AgentTurnEvent::Failed
        } else {
            match acp_transport::response(message)?["stopReason"].as_str() {
                Some("cancelled") => AgentTurnEvent::Cancelled,
                Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                    AgentTurnEvent::Completed(self.stream.last_text.clone())
                }
                _ => AgentTurnEvent::Failed,
            }
        };
        for id in std::mem::take(&mut self.pending).into_keys() {
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id));
        }
        self.terminal = Some(terminal);
        Ok(())
    }

    fn reconcile_if_settled(&mut self) {
        if self.history.is_some()
            || self.terminal.is_none()
            || (!self.settled && matches!(self.terminal, Some(AgentTurnEvent::Completed(_))))
        {
            return;
        }
        self.standard.flush();
        self.standard.finish_compactions();
        self.stream.events.extend(self.standard.events.drain(..));
        if let Some(AgentTurnEvent::Completed(message)) = &mut self.terminal {
            message.clone_from(&self.stream.last_text);
        }
        let client = self.client.clone();
        let handle = self
            .persistence()
            .expect("ACP sessions have persistence handles");
        let cwd = self.cwd.clone();
        let (sender, receiver) = oneshot::channel();
        self.history = Some(receiver);
        self.history_task = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let _ = sender.send(super::history::inspect(&client, &handle, &cwd).await);
        })));
    }
}

impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "omp"
    }
    fn control_settings(&self, requested: &StoredAgentConfig) -> Option<Value> {
        let mut features = config::features(&self.options, requested);
        if requested.model != self.initial.model {
            features.retain(|feature| feature["id"] != "fastMode" && feature["id"] != "slowMode");
        }
        Some(json!({"availableModes":config::modes(&self.options),"features":features}))
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        native_config::validate(config)?;
        Ok(())
    }
    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.transport.diagnostics.failure()
    }
    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.transport.diagnostics.message().map(Cow::Owned)
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Ok(config::runtime(&self.id, &self.options)) })
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.children.list()
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: "omp".to_owned(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([
                ("cwd".to_owned(), json!(self.cwd)),
                ("transport".to_owned(), json!("acp")),
                (
                    "empty".to_owned(),
                    json!(!self.submitted && self.stream.is_empty()),
                ),
                ("createdAt".to_owned(), json!(self.created_at)),
                ("inputIds".to_owned(), json!(self.stream.input_ids)),
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
    fn steer_turn<'a>(&'a mut self, turn: &'a str, text: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move { self.steer_input(turn, &AgentPrompt::text(text)).await })
    }
    fn steer_input<'a>(
        &'a mut self,
        turn: &'a str,
        prompt: &'a AgentPrompt,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            prompt.validate()?;
            if self.closed
                || self.terminal.is_some()
                || self.extensions["steering"]["supported"] != true
                || self
                    .active
                    .as_ref()
                    .is_none_or(|(active, _)| active != turn)
                || prompt.output_schema.is_some()
            {
                return Err(AgentSessionError::Rejected);
            }
            let result = self
                .transport
                .request_during_turn(
                    "_session/steering",
                    json!({"sessionId":self.id,"prompt":prompt.blocks()?}),
                )
                .await?;
            if result["outcome"] == "injected" {
                self.stream
                    .pending_inputs
                    .push_back(prompt.client_message_id.clone());
                Ok(())
            } else {
                Err(AgentSessionError::Rejected)
            }
        })
    }
    fn cancel_turn<'a>(&'a mut self, turn: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.closed
                || self.history.is_some()
                || self
                    .active
                    .as_ref()
                    .is_none_or(|(active, _)| active != turn)
            {
                return Err(AgentSessionError::Rejected);
            }
            self.transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await
        })
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.pending
            .values()
            .map(|pending| pending.request.clone())
            .collect()
    }
    fn respond_permission<'a>(
        &'a mut self,
        id: &'a str,
        response: &'a Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            let pending = self.pending.get(id).ok_or(AgentSessionError::Rejected)?;
            let reply = acp_interactions::resolve(pending, response)?;
            self.transport.send(&reply).await?;
            self.pending.remove(id);
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
            Ok(())
        })
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(event) = self.stream.events.pop_front() {
            return Ok(Some(event));
        }
        if let Some(receiver) = &mut self.history {
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(oneshot::error::TryRecvError::Empty) => return Ok(None),
                Err(oneshot::error::TryRecvError::Closed) => Err(AgentSessionError::Failed),
            };
            self.history = None;
            self.history_task = None;
            let history = result.inspect_err(|_| {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "acp_history"),
                    "Ait could not read OMP native history after the ACP turn.",
                    "settle",
                );
            })?;
            self.stream.pending_inputs.clear();
            self.stream
                .events
                .push_back(AgentTurnEvent::History(history.entries));
            self.stream.events.push_back(
                self.terminal
                    .take()
                    .expect("history settles an ACP terminal event"),
            );
            self.active = None;
            return Ok(self.stream.events.pop_front());
        }
        for _ in 0..64 {
            let Some(message) = self.transport.poll()? else {
                return Ok(None);
            };
            self.consume(&message).inspect_err(|_| {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "acp_event"),
                    "Ait could not process an OMP ACP event.",
                    "consume",
                );
            })?;
            if let Some(event) = self.stream.events.pop_front() {
                return Ok(Some(event));
            }
            if self.history.is_some() {
                return Ok(None);
            }
        }
        Ok(None)
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            if self.closed {
                return Ok(());
            }
            self.closed = true;
            self.history = None;
            self.history_task = None;
            let result = if self.capabilities["sessionCapabilities"]["close"].is_object() {
                self.transport
                    .request("session/close", json!({"sessionId":self.id}))
                    .await
                    .map(|_| ())
            } else {
                Ok(())
            };
            let closed = self.transport.close().await;
            result.and(closed)
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
