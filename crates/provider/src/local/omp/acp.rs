//! OMP's optional ACP execution channel; native RPC remains the default.

mod config;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio_util::task::AbortOnDropHandle;
use uuid::Uuid;

use super::{OmpClient, config as native_config};
use crate::local::{
    acp_interactions, acp_permissions,
    acp_stream::Stream,
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
    stream: Stream,
    pending: BTreeMap<String, acp_permissions::Pending>,
    closed: bool,
    submitted: bool,
    created_at: String,
    input_ids: BTreeMap<String, Value>,
    known_inputs: BTreeSet<String>,
    pending_inputs: VecDeque<Option<String>>,
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
    if let Some(handle) = handle
        && (handle.provider != "omp"
            || handle.session_id.is_empty()
            || handle
                .metadata
                .as_ref()
                .and_then(|values| values.get("cwd"))
                .and_then(Value::as_str)
                != Some(spec.cwd.as_str()))
    {
        return Err(AgentSessionError::Rejected);
    }
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
    let known_inputs = previous_history
        .as_ref()
        .map(|(history, _)| {
            history
                .entries
                .iter()
                .filter(|entry| entry.item["type"] == "user_message")
                .map(|entry| entry.key.clone())
                .collect()
        })
        .unwrap_or_default();
    let input_ids = previous_history
        .as_ref()
        .map(|(_, stream)| stream.input_ids.clone())
        .unwrap_or_default();
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
    let mut options = crate::local::acp_config::state(&result)?;
    config::apply(&mut transport, &id, &mut options, &spec.config).await?;
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
        stream: Stream::new(client.images.clone()).with_background_tools(),
        pending: BTreeMap::new(),
        closed: false,
        submitted: resuming,
        created_at: previous
            .as_ref()
            .and_then(|journal| journal.header["timestamp"].as_str())
            .map_or_else(|| chrono::Utc::now().to_rfc3339(), str::to_owned),
        input_ids,
        known_inputs,
        pending_inputs: VecDeque::new(),
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
        .args(["--mode", "acp"])
        .envs(client.environment.entries());
    if ephemeral {
        command.arg("--no-session");
    }
    if let Some(prompt) = &spec.config.system_prompt {
        command.args(["--append-system-prompt", prompt]);
    }
    if let Some(mode) = spec
        .config
        .mode_id
        .as_deref()
        .filter(|mode| !matches!(*mode, "default" | "plan"))
    {
        command.args(["--approval-mode", native_config::approval_mode(mode)]);
    }
    let mut transport = Transport::spawn(command, &spec.cwd, client.deadline, "Oh My Pi")?;
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{"elicitation":{"form":{}},"_meta":{"sessionSettled":true}},
        "clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1 || initialized["_meta"]["sessionSettled"]["version"] != 1
    {
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
    pub(super) fn draft_features(&self) -> Vec<Value> {
        vec![native_config::transport_feature(&self.initial)]
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
        self.transport.diagnostics.clear();
        config::apply(&mut self.transport, &self.id, &mut self.options, config).await?;
        let blocks = prompt.blocks()?;
        let text = blocks
            .iter()
            .map(|block| {
                if block["type"] == "image" {
                    self.client.images.render(block)
                } else {
                    Ok(block["text"].as_str().unwrap_or_default().to_owned())
                }
            })
            .collect::<Result<Vec<_>, AgentSessionError>>()?
            .join("\n\n");
        let turn = Uuid::new_v4().to_string();
        let rpc = self
            .transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"prompt":blocks,"_meta":{"turnId":turn}}),
            )
            .await?;
        self.stream.begin(turn.clone());
        self.settled = false;
        let key = format!("native:omp:acp:{}:user:{turn}", self.id);
        let mut item = json!({"type":"user_message","messageId":key,"text":text});
        if let Some(id) = &prompt.client_message_id {
            item["clientMessageId"] = json!(id);
        }
        self.stream.events.push_back(AgentTurnEvent::Timeline(
            crate::protocol::timeline::NativeItem {
                key,
                turn_id: Some(turn.clone()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                item,
            },
        ));
        self.pending_inputs
            .push_back(prompt.client_message_id.clone());
        self.active = Some((turn.clone(), rpc));
        self.submitted = true;
        self.initial.clone_from(config);
        Ok(turn)
    }

    fn consume(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        match message["method"].as_str() {
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
                    self.options = crate::local::acp_config::state(update)?;
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                            &self.id,
                            &self.options,
                        )));
                } else if self.active.is_some() && update["_meta"]["providerError"] != true {
                    // The final response carries typed native errors; project those only
                    // through the bounded, redacted failure path rather than answer text.
                    self.stream.update(update)?;
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
            None => {
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
                            AgentTurnEvent::Completed(self.stream.last_message.clone())
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
            }
        }
        self.reconcile_if_settled();
        Ok(())
    }

    fn reconcile_if_settled(&mut self) {
        if self.history.is_some()
            || self.terminal.is_none()
            || (!self.settled && matches!(self.terminal, Some(AgentTurnEvent::Completed(_))))
        {
            return;
        }
        self.stream.flush();
        self.stream.finish_compactions();
        if let Some(AgentTurnEvent::Completed(message)) = &mut self.terminal {
            message.clone_from(&self.stream.last_message);
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
    fn control_settings(&self, _config: &StoredAgentConfig) -> Option<Value> {
        Some(json!({"availableModes":config::modes(&self.options),"features":[]}))
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        native_config::validate(config)?;
        if !native_config::is_acp(config)?
            || self.initial.system_prompt != config.system_prompt
            || (config
                .mode_id
                .as_deref()
                .is_some_and(|mode| !matches!(mode, "default" | "plan"))
                && self.initial.mode_id != config.mode_id)
        {
            return Err(AgentSessionError::Rejected);
        }
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
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: "omp".to_owned(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([
                ("cwd".to_owned(), json!(self.cwd)),
                ("transport".to_owned(), json!("acp")),
                ("empty".to_owned(), json!(!self.submitted)),
                ("createdAt".to_owned(), json!(self.created_at)),
                ("inputIds".to_owned(), json!(self.input_ids)),
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
                self.pending_inputs
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
            let mut history = result.inspect_err(|_| {
                self.transport.diagnostics.observe_failure(
                    Diagnostic::new(Category::Protocol, "acp_history"),
                    "Ait could not read OMP native history after the ACP turn.",
                    "settle",
                );
            })?;
            for entry in &mut history.entries {
                if entry.item["type"] == "user_message"
                    && self.known_inputs.insert(entry.key.clone())
                {
                    if self.known_inputs.len() > 4096 {
                        return Err(AgentSessionError::Failed);
                    }
                    if let Some(Some(id)) = self.pending_inputs.pop_front() {
                        self.input_ids.insert(entry.key.clone(), json!(id));
                    }
                }
                if let Some(id) = self.input_ids.get(&entry.key) {
                    entry.item["clientMessageId"] = id.clone();
                }
            }
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
