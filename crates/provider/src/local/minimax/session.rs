use std::borrow::Cow;
use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MiniMaxClient, PROVIDER, config};
use crate::local::acp_streaming::Stream;
use crate::local::{
    acp_config::text,
    acp_permissions,
    acp_transport::{self as transport, Transport},
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

/// One native ACP session with bounded control requests and a single active prompt.
#[derive(Debug)]
pub(super) struct Session {
    transport: Transport,
    id: String,
    cwd: String,
    initial_config: StoredAgentConfig,
    /// Last confirmed native configuration, including normalized workflow modes.
    pub(super) options: Value,
    capabilities: Value,
    active: Option<(String, String)>,
    stream: Stream,
    images: crate::local::images::ImageStore,
    permissions: BTreeMap<String, acp_permissions::Pending>,
    closed: bool,
}

/// Create or resume a native ACP writer; rejects foreign handles and unsupported settings.
/// Uses `client` launch settings and `spec`; `handle` selects native resume when present.
/// Returns the initialized session, or a launch, authentication or protocol error.
pub(super) async fn open(
    client: &MiniMaxClient,
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
    let mut command = tokio::process::Command::new(&client.program);
    command.arg("acp").envs(client.environment.entries());
    if let Some(prompt) = &spec.config.system_prompt {
        command.arg("--append-system-prompt").arg(prompt);
    }
    let mut transport = Transport::spawn(command, &spec.cwd, client.deadline, PROVIDER)?;
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{},"clientInfo":{"name":"ait","version":env!("CARGO_PKG_VERSION")}}),
        )
        .await?;
    if initialized["protocolVersion"] != 1 {
        return Err(AgentSessionError::Failed);
    }
    let capabilities = initialized["agentCapabilities"].clone();
    let mut params =
        json!({"cwd":spec.cwd,"mcpServers":crate::local::acp_config::mcp_servers(&spec.config)});
    let method = if let Some(handle) = handle {
        if !capabilities["sessionCapabilities"]["resume"].is_object() {
            return Err(AgentSessionError::Rejected);
        }
        params["sessionId"] = json!(handle.session_id);
        "session/resume"
    } else {
        "session/new"
    };
    let result = transport.request(method, params).await?;
    let id = match handle {
        Some(handle) => handle.session_id.clone(),
        None => text(&result, "sessionId")?.to_owned(),
    };
    let mut session = Session {
        transport,
        id,
        cwd: spec.cwd.clone(),
        initial_config: spec.config.clone(),
        options: config::state(&result, None)?,
        capabilities,
        active: None,
        stream: Stream::new(client.images.clone()),
        images: client.images.clone(),
        permissions: BTreeMap::new(),
        closed: false,
    };
    if let Err(error) = config::apply(
        &mut session.transport,
        &session.id,
        &mut session.options,
        &spec.config,
    )
    .await
    {
        let _ = session.close().await;
        return Err(error);
    }
    Ok(session)
}

impl Session {
    /// Verify advertised models in this disposable discovery session without running prompts.
    /// Returns confirmed model controls; rejects transport failures instead of hiding them.
    pub(super) async fn discover(
        &mut self,
    ) -> Result<crate::protocol::provider::Details, AgentSessionError> {
        let mut details = config::details(&self.options)?;
        for model in &mut details.models {
            let selected = StoredAgentConfig {
                model: Some(text(model, "id")?.to_owned()),
                ..StoredAgentConfig::default()
            };
            match config::apply(&mut self.transport, &self.id, &mut self.options, &selected).await {
                Ok(()) => config::update_thinking(model, &self.options)?,
                Err(AgentSessionError::Rejected) => {
                    model["isSelectable"] = json!(false);
                    model["isDefault"] = json!(false);
                }
                Err(error) => return Err(error),
            }
            // Configuration notifications belong to this disposable session. Drain them
            // between probes so a large catalog cannot fill the transport's bounded queue.
            while let Some(message) = self.transport.poll()? {
                self.consume(&message)?;
            }
            self.stream.events.clear();
        }
        Ok(details)
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
            || config.system_prompt != self.initial_config.system_prompt
        {
            return Err(AgentSessionError::Rejected);
        }
        prompt.validate()?;
        let blocks = prompt.blocks()?;
        let sources = prompt
            .images
            .iter()
            .map(|image| {
                self.images
                    .source(&json!({"data":image.data,"mimeType":image.mime_type}))
                    .map(Cow::into_owned)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.transport.diagnostics.clear();
        config::apply(&mut self.transport, &self.id, &mut self.options, config).await?;
        let user_text = blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let turn = Uuid::new_v4().to_string();
        let rpc_id = self
            .transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"prompt":blocks}),
            )
            .await?;
        self.stream.begin(turn.clone());
        let mut item = json!({
            "type":"user_message",
            "messageId":format!("native:{turn}:user"),
            "text":user_text
        });
        if !sources.is_empty() {
            item["imageSources"] = json!(sources);
        }
        if let Some(id) = &prompt.client_message_id {
            item["clientMessageId"] = json!(id);
        }
        self.stream.events.push_back(AgentTurnEvent::Timeline(
            crate::protocol::timeline::NativeItem {
                key: format!("native:{turn}:user"),
                turn_id: Some(turn.clone()),
                timestamp: chrono::Utc::now().to_rfc3339(),
                item,
            },
        ));
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
        if self.stream.events.len() >= 4096 {
            return Err(AgentSessionError::Failed);
        }
        match message["method"].as_str() {
            Some("session/update") => {
                if message["params"]["sessionId"] != self.id {
                    return Err(AgentSessionError::Failed);
                }
                let update = &message["params"]["update"];
                if update["sessionUpdate"] == "config_option_update" {
                    self.options = config::state(update, Some(&self.options))?;
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                            &self.id,
                            &self.options,
                        )));
                } else if update["sessionUpdate"] == "current_mode_update" {
                    config::update_mode(&mut self.options, text(update, "currentModeId")?)?;
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                            &self.id,
                            &self.options,
                        )));
                } else if self.active.is_some() {
                    self.stream.update(update)?;
                }
            }
            Some("session/request_permission") => {
                if self.active.is_none()
                    || message["params"]["sessionId"] != self.id
                    || self.permissions.len() >= 128
                    || self
                        .permissions
                        .values()
                        .any(|pending| pending.rpc_id == message["id"])
                {
                    return Err(AgentSessionError::Failed);
                }
                let pending = acp_permissions::capture(message, PROVIDER)?;
                let id = text(&pending.request, "id")?.to_owned();
                self.stream
                    .events
                    .push_back(AgentTurnEvent::PermissionRequested(pending.request.clone()));
                self.permissions.insert(id, pending);
            }
            None => {
                let (_, id) = self.active.as_ref().ok_or(AgentSessionError::Failed)?;
                if message["id"] != *id {
                    return Err(AgentSessionError::Failed);
                }
                self.stream.flush();
                self.stream.finish_compactions();
                if let Some(error) = message.get("error") {
                    self.transport
                        .diagnostics
                        .observe_native(error, "rpc", "session/prompt");
                }
                let event = match transport::response(message) {
                    Ok(result) => match result["stopReason"].as_str() {
                        Some("cancelled") => AgentTurnEvent::Cancelled,
                        Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                            AgentTurnEvent::Completed(self.stream.last_message.clone())
                        }
                        _ => AgentTurnEvent::Failed,
                    },
                    Err(_) => AgentTurnEvent::Failed,
                };
                for id in std::mem::take(&mut self.permissions).into_keys() {
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::PermissionResolved(id));
                }
                if let Some(usage) = message["result"].get("usage") {
                    let usage = crate::protocol::usage::AgentUsage {
                        input_tokens: usage["inputTokens"].as_u64(),
                        output_tokens: usage["outputTokens"].as_u64(),
                        cached_input_tokens: usage["cachedReadTokens"].as_u64(),
                        ..Default::default()
                    };
                    if !usage.is_valid() {
                        return Err(AgentSessionError::Failed);
                    }
                    self.stream.events.push_back(AgentTurnEvent::Usage(usage));
                }
                self.stream.events.push_back(event);
                self.active = None;
            }
            Some(_) if message.get("id").is_some() => return Err(AgentSessionError::Failed),
            Some(_) => {}
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        if config.system_prompt != self.initial_config.system_prompt
            || config.mcp_servers != self.initial_config.mcp_servers
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(())
    }

    fn control_settings(&self, _config: &StoredAgentConfig) -> Option<Value> {
        Some(config::settings(&self.options))
    }

    fn failure_diagnostic(&self) -> Option<crate::ports::diagnostics::Diagnostic> {
        self.transport.diagnostics.failure()
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.transport.diagnostics.message().map(Cow::Owned)
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(config::runtime(&self.id, &self.options))
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([("cwd".to_owned(), json!(self.cwd))])),
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

    async fn cancel_turn(&mut self, turn_id: &str) -> Result<(), AgentSessionError> {
        if self.closed || self.active.as_ref().is_none_or(|(turn, _)| turn != turn_id) {
            return Err(AgentSessionError::Rejected);
        }
        self.transport
            .send(&json!({
                "jsonrpc":"2.0","method":"session/cancel",
                "params":{"sessionId":self.id}
            }))
            .await?;
        let result = tokio::time::timeout(self.transport.control_deadline(), async {
            while self.active.is_some() {
                let message = self.transport.receive().await?;
                self.consume(&message)?;
            }
            Ok(())
        })
        .await
        .unwrap_or(Err(AgentSessionError::Failed));
        if result.is_err() {
            self.closed = true;
            let _ = self.transport.close().await;
        }
        result
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

    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        let pending = self
            .permissions
            .get(id)
            .ok_or(AgentSessionError::Rejected)?;
        let reply = acp_permissions::resolve_tool(pending, response)?;
        self.transport.send(&reply).await?;
        self.permissions.remove(id);
        self.stream
            .events
            .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
        if response["behavior"] == "deny" && response["interrupt"] == true {
            self.transport
                .send(&json!({
                    "jsonrpc":"2.0","method":"session/cancel",
                    "params":{"sessionId":self.id}
                }))
                .await?;
        }
        Ok(())
    }

    async fn close(&mut self) -> Result<(), AgentSessionError> {
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
    }
}
