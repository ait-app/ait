use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    DeepSeekHarnessClient, PROVIDER, config, permissions,
    streaming::Stream,
    transport::{self, Transport},
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

#[derive(Debug)]
pub(super) struct Session {
    transport: Transport,
    id: String,
    cwd: String,
    initial_config: StoredAgentConfig,
    pub(super) options: Value,
    capabilities: Value,
    active: Option<(String, String)>,
    stream: Stream,
    permissions: BTreeMap<String, permissions::Pending>,
    closed: bool,
}

pub(super) async fn open(
    client: &DeepSeekHarnessClient,
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
    let mut transport = transport::spawn(client, &spec.cwd)?;
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
    let mut params = json!({"cwd":spec.cwd,"mcpServers":config::mcp_servers(&spec.config)});
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
        None => config::text(&result, "sessionId")?.to_owned(),
    };
    let mut session = Session {
        transport,
        id,
        cwd: spec.cwd.clone(),
        initial_config: spec.config.clone(),
        options: config::state(&result)?,
        capabilities,
        active: None,
        stream: Stream::new(client.images.clone()),
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
        let blocks = prompt.blocks()?;
        config::apply(&mut self.transport, &self.id, &mut self.options, config).await?;
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
                    self.options = config::state(update)?;
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
                let pending = permissions::capture(message)?;
                let id = config::text(&pending.request, "id")?.to_owned();
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
                self.stream.events.push_back(event);
                self.active = None;
            }
            Some(_) if message.get("id").is_some() => return Err(AgentSessionError::Failed),
            Some(_) => {}
        }
        Ok(())
    }
}

impl AgentSession for Session {
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
            self.transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await
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
            let reply = permissions::resolve(pending, response)?;
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
