use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::process::Command;
use uuid::Uuid;

use super::{CursorClient, PROVIDER, config, interactions};
use crate::local::acp::{
    streaming::Stream,
    text,
    transport::{self, Transport},
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

/// One Cursor ACP session, its pending approvals and turn-local timeline.
#[derive(Debug)]
pub(super) struct Session {
    transport: Transport,
    id: String,
    cwd: String,
    initial_config: StoredAgentConfig,
    pub(super) options: Value,
    pub(super) catalog: Vec<config::Model>,
    commands: Option<Vec<Value>>,
    capabilities: Value,
    active: Option<(String, String)>,
    stream: Stream,
    permissions: BTreeMap<String, interactions::Pending>,
    closed: bool,
}

/// Open or load Cursor using `client`, `spec` and optional handle; reject foreign handles or failed ACP negotiation.
pub(super) async fn open(
    client: &CursorClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    if let Some(handle) = handle
        && (handle.provider != PROVIDER
            || handle.session_id.is_empty()
            || handle.session_id.len() > 1024
            || handle.session_id.chars().any(char::is_control)
            || handle
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cwd"))
                .and_then(Value::as_str)
                != Some(&spec.cwd))
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut command = Command::new(&client.program);
    command
        .arg("acp")
        .current_dir(&spec.cwd)
        .envs(client.environment.entries());
    let mut transport = Transport::spawn(command, client.deadline)?;
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{"_meta":{"parameterizedModelPicker":true}},"clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1 {
        return Err(AgentSessionError::Failed);
    }
    if initialized["authMethods"]
        .as_array()
        .is_some_and(|methods| methods.iter().any(|method| method["id"] == "cursor_login"))
    {
        transport
            .request("authenticate", json!({"methodId":"cursor_login"}))
            .await?;
    }
    let capabilities = initialized["agentCapabilities"].clone();
    let mut params = json!({"cwd":spec.cwd,"mcpServers":[]});
    let method = if let Some(handle) = handle {
        params["sessionId"] = json!(handle.session_id);
        if capabilities["loadSession"] == true {
            "session/load"
        } else if capabilities["sessionCapabilities"]["resume"].is_object() {
            "session/resume"
        } else {
            return Err(AgentSessionError::Rejected);
        }
    } else {
        "session/new"
    };
    let result = transport.request(method, params).await?;
    let id = match handle {
        Some(handle) => {
            if result
                .get("sessionId")
                .is_some_and(|id| id != &handle.session_id)
            {
                return Err(AgentSessionError::Failed);
            }
            handle.session_id.clone()
        }
        None => text(&result, "sessionId")?.to_owned(),
    };
    let catalog = config::catalog(
        &transport
            .request("cursor/list_available_models", json!({}))
            .await?,
    )?;
    let mut options = config::state(&result)?;
    config::expand_models(&mut options, &catalog);
    let mut session = Session {
        transport,
        id,
        cwd: spec.cwd.clone(),
        initial_config: spec.config.clone(),
        options,
        catalog,
        commands: None,
        capabilities,
        active: None,
        stream: Stream::new(client.images.clone()),
        permissions: BTreeMap::new(),
        closed: false,
    };
    if let Err(error) = async {
        session.drain_idle()?;
        session.configure(&spec.config).await
    }
    .await
    {
        let _ = session.close().await;
        return Err(error);
    }
    Ok(session)
}

impl Session {
    fn drain_idle(&mut self) -> Result<(), AgentSessionError> {
        for _ in 0..128 {
            let Some(message) = self.transport.poll()? else {
                return Ok(());
            };
            self.consume(&message)?;
        }
        Err(AgentSessionError::Failed)
    }

    async fn configure(&mut self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::apply(
            &mut self.transport,
            &self.id,
            &mut self.options,
            &self.catalog,
            config,
        )
        .await?;
        self.drain_idle()
    }

    /// Wait at most ten seconds for native slash commands; return an empty list if no update arrives.
    /// Fails on invalid notifications or disconnected transport.
    pub(super) async fn commands(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        let result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while self.commands.is_none() {
                let message = self.transport.receive().await?;
                self.consume(&message)?;
            }
            Ok(self.commands.clone().unwrap_or_default())
        })
        .await;
        result.unwrap_or_else(|_| Ok(Vec::new()))
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        if self.closed
            || self.active.is_some()
            || !self.permissions.is_empty()
            || config.mcp_servers != self.initial_config.mcp_servers
            || prompt.output_schema.is_some()
            || (!prompt.images.is_empty()
                && self.capabilities["promptCapabilities"]["image"] != true)
        {
            return Err(AgentSessionError::Rejected);
        }
        self.drain_idle()?;
        let blocks = prompt.blocks()?;
        self.configure(config).await?;
        let turn = Uuid::new_v4().to_string();
        let rpc_id = self
            .transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"prompt":blocks}),
            )
            .await?;
        self.stream.begin(turn.clone());
        self.active = Some((turn.clone(), rpc_id));
        self.stream
            .events
            .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                &self.id,
                &self.options,
            )));
        Ok(turn)
    }

    fn consume(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        match message["method"].as_str() {
            Some("session/update") => {
                if message["params"]["sessionId"] != self.id {
                    return Err(AgentSessionError::Failed);
                }
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "config_option_update" {
                    config::merge(&mut self.options, update)?;
                    config::expand_models(&mut self.options, &self.catalog);
                    if self.active.is_some() {
                        self.stream
                            .events
                            .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                                &self.id,
                                &self.options,
                            )));
                    }
                } else if update["sessionUpdate"] == "available_commands_update" {
                    self.commands = Some(commands(update)?);
                } else if update["sessionUpdate"] == "current_mode_update" {
                    let mode = text(update, "currentModeId")?;
                    if let Some(options) = self.options.as_array_mut() {
                        for option in options
                            .iter_mut()
                            .filter(|option| option["category"] == "mode")
                        {
                            option["currentValue"] = json!(mode);
                        }
                    }
                    if self.active.is_some() {
                        self.stream
                            .events
                            .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                                &self.id,
                                &self.options,
                            )));
                    }
                } else if self.active.is_some() {
                    self.stream.update(update)?;
                }
            }
            Some("session/request_permission" | "cursor/ask_question" | "cursor/create_plan") => {
                if self.active.is_none()
                    || (message["params"].get("sessionId").is_some()
                        && message["params"]["sessionId"] != self.id)
                    || (message["method"] == "session/request_permission"
                        && message["params"]["sessionId"] != self.id)
                    || self.permissions.len() >= 128
                    || self
                        .permissions
                        .values()
                        .any(|pending| pending.rpc_id == message["id"])
                {
                    return Err(AgentSessionError::Failed);
                }
                let pending = interactions::Pending::capture(message)?;
                let id = text(&pending.request, "id")?.to_owned();
                self.stream
                    .events
                    .push_back(AgentTurnEvent::PermissionRequested(pending.request.clone()));
                self.permissions.insert(id, pending);
            }
            None => self.complete(message)?,
            Some(_) if message.get("id").is_some() => return Err(AgentSessionError::Failed),
            Some(_) => {}
        }
        Ok(())
    }

    fn complete(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        let (_, id) = self.active.as_ref().ok_or(AgentSessionError::Failed)?;
        if message["id"] != *id {
            return Err(AgentSessionError::Failed);
        }
        let result = transport::response(message);
        if let Ok(result) = &result {
            self.stream.prompt_usage(result)?;
        }
        let unfinished = self.stream.has_unfinished_tools() || !self.permissions.is_empty();
        let event = match result {
            Ok(result) => match result["stopReason"].as_str() {
                Some("cancelled") => AgentTurnEvent::Cancelled,
                Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                    if unfinished {
                        AgentTurnEvent::Failed
                    } else {
                        self.stream.flush();
                        AgentTurnEvent::Completed(self.stream.last_message.clone())
                    }
                }
                _ => AgentTurnEvent::Failed,
            },
            Err(_) => AgentTurnEvent::Failed,
        };
        self.stream.finish(if event == AgentTurnEvent::Cancelled {
            "Turn cancelled"
        } else {
            "Turn ended before tool completed"
        })?;
        for id in std::mem::take(&mut self.permissions).into_keys() {
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id));
        }
        self.stream.events.push_back(event);
        self.active = None;
        Ok(())
    }
}

impl AgentSession for Session {
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        Some(
            json!({"availableModes":config::details(&self.options, &self.catalog).ok()?.modes,
            "features":config::features(&self.options, &self.catalog, config),
            "capabilities":{"supportsStreaming":true,"supportsReasoningStream":true,
                "supportsDynamicModes":true,"supportsMcpServers":false}}),
        )
    }

    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate_selection(&self.options, &self.catalog, config)
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Ok(config::runtime(&self.id, &self.options)) })
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([("cwd".to_owned(), json!(self.cwd))])),
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

    fn cancel_turn<'a>(&'a mut self, turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.closed || self.active.as_ref().is_none_or(|(turn, _)| turn != turn_id) {
                return Err(AgentSessionError::Rejected);
            }
            self.transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await?;
            for (id, pending) in std::mem::take(&mut self.permissions) {
                self.transport.send(&pending.cancelled()).await?;
                self.stream
                    .events
                    .push_back(AgentTurnEvent::PermissionResolved(id));
            }
            Ok(())
        })
    }

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(event) = self.stream.events.pop_front() {
            return Ok(Some(event));
        }
        for _ in 0..128 {
            let Some(message) = self.transport.poll()? else {
                return Ok(None);
            };
            self.consume(&message)?;
            if let Some(event) = self.stream.events.pop_front() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }

    fn pending_permissions(&self) -> Vec<Value> {
        self.permissions
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
            let pending = self
                .permissions
                .get(id)
                .ok_or(AgentSessionError::Rejected)?;
            let reply = pending.resolve(response)?;
            self.transport.send(&reply).await?;
            self.permissions.remove(id);
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
            if response["behavior"] == "deny" && response["interrupt"] == true {
                self.transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await?;
            }
            Ok(())
        })
    }

    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            if self.closed {
                return Ok(());
            }
            let result = if self.capabilities["sessionCapabilities"]["close"].is_object() {
                self.transport
                    .request("session/close", json!({"sessionId":self.id}))
                    .await
                    .map(|_| ())
            } else {
                Ok(())
            };
            let stopped = self.transport.close().await;
            self.closed = true;
            result.and(stopped)
        })
    }
}

fn commands(update: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let commands = update["availableCommands"]
        .as_array()
        .filter(|commands| commands.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    let mut names = std::collections::BTreeSet::new();
    commands
        .iter()
        .map(|command| {
            let name = text(command, "name")?;
            if !names.insert(name) {
                return Err(AgentSessionError::Failed);
            }
            let description = command["description"]
                .as_str()
                .filter(|description| description.len() <= 16384)
                .ok_or(AgentSessionError::Failed)?;
            let hint = command
                .pointer("/input/hint")
                .filter(|hint| !hint.is_null())
                .map(|hint| {
                    hint.as_str()
                        .filter(|hint| hint.len() <= 1024)
                        .ok_or(AgentSessionError::Failed)
                })
                .transpose()?
                .unwrap_or("");
            Ok(json!({"name":name,"description":description,"argumentHint":hint,"kind":"command"}))
        })
        .collect()
}
