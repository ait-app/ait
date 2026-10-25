use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{aliases, mapping};
use crate::local::omp::{OmpClient, config, history, session};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};

const METHODS: &[&str] = &[
    "initialize",
    "session/new",
    "session/load",
    "session/resume",
    "session/prompt",
    "session/cancel",
    "session/set_mode",
    "session/set_config_option",
    "session/close",
    "_session/steering",
    "_ait/runtime",
    "_ait/respond_permission",
    "_omp/rpc",
    "_omp/respond",
];

#[derive(Debug, Default)]
struct ClientCapabilities {
    ait: bool,
    raw_extensions: bool,
    elicitation: bool,
}

#[derive(Debug)]
/// Own one official OMP runtime and translate correlated ACP requests and native events.
pub(super) struct Engine {
    client: OmpClient,
    native: Option<session::Session>,
    initialized: bool,
    capabilities: ClientCapabilities,
    id: String,
    cwd: String,
    config: StoredAgentConfig,
    active: Option<(Value, String)>,
    callbacks: BTreeMap<String, Value>,
    native_controls: BTreeMap<String, Value>,
    mapping: mapping::Projection,
    outgoing: Vec<Value>,
    closed: bool,
}

impl Engine {
    /// Create an uninitialized bridge for `client`, without starting OMP or submitting input.
    pub(super) fn new(client: OmpClient) -> Self {
        Self {
            client,
            native: None,
            initialized: false,
            capabilities: ClientCapabilities::default(),
            id: String::new(),
            cwd: String::new(),
            config: StoredAgentConfig::default(),
            active: None,
            callbacks: BTreeMap::new(),
            native_controls: BTreeMap::new(),
            mapping: mapping::Projection::default(),
            outgoing: Vec::new(),
            closed: false,
        }
    }

    /// Borrow the sole native owner for cached host persistence and diagnostic facts.
    pub(super) fn native(&self) -> Option<&session::Session> {
        self.native.as_ref()
    }

    fn native_mut(&mut self) -> Result<&mut session::Session, AgentSessionError> {
        self.native.as_mut().ok_or(AgentSessionError::Rejected)
    }

    /// Process one JSON-RPC `message`; return responses/notifications, with failures on the wire.
    pub(super) async fn dispatch(&mut self, message: Value) -> Vec<Value> {
        if message["jsonrpc"] != "2.0" || !message.is_object() {
            return vec![
                json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32600,"message":"Invalid Request"}}),
            ];
        }
        if message.get("method").is_none() && message.get("id").is_some() {
            if let Some(id) = message["id"].as_str()
                && let Some(request) = self.callbacks.get(id).cloned()
            {
                let response = mapping::answer(&request, &message["result"])
                    .unwrap_or_else(|_| json!({"behavior":"deny"}));
                if let Some(native) = &mut self.native {
                    let request_id = request["id"].as_str().unwrap_or_default();
                    if native
                        .respond_permission(request_id, &response)
                        .await
                        .is_err()
                    {
                        let _ = native
                            .respond_permission(request_id, &json!({"behavior":"deny"}))
                            .await;
                    }
                    self.callbacks.remove(id);
                }
            }
            return Vec::new();
        }
        let Some(method) = message["method"].as_str() else {
            return vec![
                json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32600,"message":"Invalid Request"}}),
            ];
        };
        if !METHODS.contains(&method) {
            return message
                .get("id")
                .map(|id| {
                    json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32601,"message":"Method not found"}})
                })
                .into_iter()
                .collect();
        }
        let result = self.execute(method, &message).await;
        if let Some(id) = message.get("id") {
            match result {
                Ok(Some(result)) => self
                    .outgoing
                    .push(json!({"jsonrpc":"2.0","id":id,"result":result})),
                Ok(None) => {}
                Err(error) => {
                    let detail = self
                        .native()
                        .and_then(AgentSession::failure_message)
                        .map_or_else(|| error.to_string(), std::borrow::Cow::into_owned);
                    self.outgoing.push(json!({"jsonrpc":"2.0","id":id,"error":{
                        "code":if error==AgentSessionError::Rejected {-32602} else {-32603},
                        "message":detail}}));
                }
            }
        }
        std::mem::take(&mut self.outgoing)
    }

    fn initialize(&mut self, params: &Value) -> Result<Option<Value>, AgentSessionError> {
        if self.initialized || params["protocolVersion"] != 1 {
            return Err(AgentSessionError::Rejected);
        }
        self.initialized = true;
        self.capabilities.ait = params["clientCapabilities"]["_meta"]["ait"] == 1;
        self.capabilities.raw_extensions =
            params["clientCapabilities"]["_meta"]["ompNative"] == true;
        self.capabilities.elicitation =
            params["clientCapabilities"]["elicitation"]["form"].is_object();
        Ok(Some(
            json!({"protocolVersion":1,"agentInfo":{"name":"ait-omp-acp","version":env!("CARGO_PKG_VERSION")},
                "agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true,"embeddedContext":true},
                    "sessionCapabilities":{"resume":{},"close":{}}},
                "authMethods":[],"_meta":{"ait":{"version":1},"ompNative":{"version":1},
                    "steering":{"supported":true},"backend":{"name":"omp","protocol":"rpc"}}}),
        ))
    }

    async fn execute(
        &mut self,
        method: &str,
        message: &Value,
    ) -> Result<Option<Value>, AgentSessionError> {
        let params = &message["params"];
        if method == "initialize" {
            return self.initialize(params);
        }
        if !self.initialized {
            return Err(AgentSessionError::Rejected);
        }
        if matches!(method, "session/new" | "session/resume" | "session/load") {
            return self.open(method, params).await.map(Some);
        }
        if params["sessionId"] != self.id || self.native.is_none() {
            return Err(AgentSessionError::Rejected);
        }
        match method {
            "session/prompt" => self.start_prompt(message, params).await,
            "session/cancel" => {
                let turn = self
                    .active
                    .as_ref()
                    .map(|(_, turn)| turn.clone())
                    .ok_or(AgentSessionError::Rejected)?;
                if params["_meta"]["ait"]["turnId"]
                    .as_str()
                    .is_some_and(|id| id != turn)
                {
                    return Err(AgentSessionError::Rejected);
                }
                self.native_mut()?.cancel_turn(&turn).await?;
                Ok(Some(json!({})))
            }
            "_session/steering" => self.steer(params).await,
            "session/set_config_option" | "session/set_mode" => {
                self.set_config(method, params).await
            }
            "_ait/runtime" if self.capabilities.ait => {
                let runtime = self.native_mut()?.runtime_info().await?;
                serde_json::to_value(runtime)
                    .map(Some)
                    .map_err(|_| AgentSessionError::Failed)
            }
            "_ait/respond_permission" if self.capabilities.ait => {
                let id = config::text(params, "id")?;
                self.native_mut()?
                    .respond_permission(id, &params["response"])
                    .await?;
                self.callbacks.retain(|_, request| request["id"] != id);
                Ok(Some(json!({})))
            }
            "_omp/rpc" if self.capabilities.raw_extensions => {
                if self.native_controls.len() >= 128 {
                    return Err(AgentSessionError::Rejected);
                }
                let kind = config::text(params, "command")?;
                let id = self
                    .native_mut()?
                    .begin_native_control(
                        kind,
                        params
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| json!({})),
                    )
                    .await?;
                self.native_controls.insert(id, message["id"].clone());
                Ok(None)
            }
            "_omp/respond" if self.capabilities.raw_extensions => {
                self.native_mut()?.native_response(&params["frame"]).await?;
                if let Some(id) = params["frame"]["id"].as_str() {
                    self.callbacks.retain(|_, request| request["id"] != id);
                }
                Ok(Some(json!({})))
            }
            "session/close" => {
                if !self.closed {
                    self.save_alias()?;
                    self.native_mut()?.close().await?;
                    if let Some((id, _)) = self.active.take() {
                        let meta = if self.capabilities.ait {
                            json!({"ait":{"event":AgentTurnEvent::Cancelled}})
                        } else {
                            json!({})
                        };
                        self.outgoing.push(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled","_meta":meta}}));
                    }
                    self.closed = true;
                }
                Ok(Some(json!({})))
            }
            _ => Err(AgentSessionError::Rejected),
        }
    }

    async fn steer(&mut self, params: &Value) -> Result<Option<Value>, AgentSessionError> {
        let turn = self
            .active
            .as_ref()
            .map(|(_, turn)| turn.clone())
            .ok_or(AgentSessionError::Rejected)?;
        if params["_meta"]["ait"]["turnId"]
            .as_str()
            .is_some_and(|id| id != turn)
        {
            return Err(AgentSessionError::Rejected);
        }
        let prompt = mapping::prompt(params, self.capabilities.ait)?;
        self.native_mut()?.steer_input(&turn, &prompt).await?;
        Ok(Some(json!({"outcome":"injected"})))
    }

    async fn start_prompt(
        &mut self,
        message: &Value,
        params: &Value,
    ) -> Result<Option<Value>, AgentSessionError> {
        if message.get("id").is_none() || self.active.is_some() || self.closed {
            return Err(AgentSessionError::Rejected);
        }
        let prompt = mapping::prompt(params, self.capabilities.ait)?;
        if self.capabilities.ait && !params["_meta"]["ait"]["config"].is_null() {
            let config = serde_json::from_value(params["_meta"]["ait"]["config"].clone())
                .map_err(|_| AgentSessionError::Rejected)?;
            config::validate(&config)?;
            self.config = config;
        }
        let config = self.config.clone();
        let turn = self.native_mut()?.start_input(&prompt, &config).await?;
        self.mapping = mapping::Projection::default();
        self.active = Some((message["id"].clone(), turn.clone()));
        if self.capabilities.ait {
            self.notify(
                "_ait/prompt_accepted",
                json!({"sessionId":self.id,"turnId":turn}),
            );
        }
        Ok(None)
    }

    async fn set_config(
        &mut self,
        method: &str,
        params: &Value,
    ) -> Result<Option<Value>, AgentSessionError> {
        if self.active.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        let mut config = self.config.clone();
        let (id, value) = if method == "session/set_mode" {
            ("mode", &params["modeId"])
        } else {
            (config::text(params, "configId")?, &params["value"])
        };
        match id {
            "model" => {
                config.model = Some(config::text(&json!({"value":value}), "value")?.to_owned());
            }
            "thinking" => {
                config.thinking_option_id =
                    Some(config::text(&json!({"value":value}), "value")?.to_owned());
            }
            "mode" => {
                config.mode_id = if value == "native" {
                    None
                } else {
                    Some(config::text(&json!({"value":value}), "value")?.to_owned())
                }
            }
            "autoCompaction" | "autoRetry" | "fastMode" | "slowMode" => {
                let enabled = match value.as_str() {
                    Some("true") => true,
                    Some("false") => false,
                    _ => return Err(AgentSessionError::Rejected),
                };
                config
                    .feature_values
                    .get_or_insert_default()
                    .insert(id.to_owned(), json!(enabled));
            }
            _ => return Err(AgentSessionError::Rejected),
        }
        self.native_mut()?.configure(&config).await?;
        self.config = config;
        let options = self.options().await?;
        self.notify("session/update",json!({"sessionId":self.id,"update":{"sessionUpdate":"config_option_update","configOptions":options}}));
        Ok(Some(json!({"configOptions":options})))
    }

    async fn open(&mut self, method: &str, params: &Value) -> Result<Value, AgentSessionError> {
        if self.native.is_some()
            || params["mcpServers"]
                .as_array()
                .is_none_or(|servers| !servers.is_empty())
        {
            return Err(AgentSessionError::Rejected);
        }
        let config = if self.capabilities.ait && params["_meta"]["ait"].get("config").is_some() {
            serde_json::from_value(params["_meta"]["ait"]["config"].clone())
                .map_err(|_| AgentSessionError::Rejected)?
        } else {
            StoredAgentConfig::default()
        };
        let alias = if method != "session/new" && !self.capabilities.ait {
            aliases::load(
                &self.client,
                config::text(params, "sessionId")?,
                config::text(params, "cwd")?,
            )?
        } else {
            None
        };
        let spec = AgentSessionSpec {
            provider: "omp".to_owned(),
            cwd: config::text(params, "cwd")?.to_owned(),
            config: alias.as_ref().map_or(config, |alias| alias.config.clone()),
        };
        let handle = if let Some(alias) = alias {
            Some(alias.handle)
        } else if method == "session/new" {
            None
        } else {
            let handle =
                if self.capabilities.ait && !params["_meta"]["ait"]["persistence"].is_null() {
                    serde_json::from_value::<AgentPersistenceHandle>(
                        params["_meta"]["ait"]["persistence"].clone(),
                    )
                    .map_err(|_| AgentSessionError::Rejected)?
                } else {
                    AgentPersistenceHandle {
                        provider: "omp".to_owned(),
                        session_id: config::text(params, "sessionId")?.to_owned(),
                        native_handle: None,
                        metadata: None,
                    }
                };
            if params["sessionId"] != handle.session_id {
                return Err(AgentSessionError::Rejected);
            }
            Some(handle)
        };
        let mut native = session::open(&self.client, &spec, handle.as_ref()).await?;
        native.forward_native_events(self.capabilities.raw_extensions);
        self.id = if method != "session/new" && !self.capabilities.ait {
            config::text(params, "sessionId")?.to_owned()
        } else {
            native
                .persistence()
                .ok_or(AgentSessionError::Failed)?
                .session_id
        };
        self.cwd = spec.cwd;
        self.config = spec.config;
        self.native = Some(native);
        let options = self.options().await?;
        if method == "session/load"
            && let Some(handle) = &handle
        {
            let transcript = history::inspect(&self.client, handle, &self.cwd).await?;
            for entry in transcript.entries {
                self.outgoing.extend(
                    self.mapping
                        .update(&self.id, &AgentTurnEvent::Timeline(entry))?,
                );
            }
        }
        let commands = self
            .native_mut()?
            .native_request("get_available_commands", json!({}))
            .await?;
        let commands = config::commands(&commands)?;
        self.notify("session/update",json!({"sessionId":self.id,"update":{"sessionUpdate":"available_commands_update",
            "availableCommands":commands.iter().map(|command|json!({"name":command["name"],"description":command["description"],
                "input":{"hint":command["argumentHint"]},"_meta":{"kind":command["kind"]}})).collect::<Vec<_>>()}}));
        Ok(json!({"sessionId":self.id,"configOptions":options}))
    }

    async fn options(&mut self) -> Result<Value, AgentSessionError> {
        let runtime = self.native_mut()?.runtime_info().await?;
        let models = self
            .native_mut()?
            .native_request("get_available_models", json!({}))
            .await?;
        let models = models["models"]
            .as_array()
            .filter(|models| models.len() <= 8192)
            .ok_or(AgentSessionError::Failed)?;
        let choices=models.iter().map(|model|Ok(json!({"value":json!([config::text(model,"provider")?,config::text(model,"id")?]).to_string(),
            "name":model["name"].as_str().unwrap_or(config::text(model,"id")?)}))).collect::<Result<Vec<_>,AgentSessionError>>()?;
        let levels = self
            .native_mut()?
            .native_request("get_available_thinking_levels", json!({}))
            .await?;
        let levels = levels["levels"]
            .as_array()
            .filter(|levels| levels.len() <= 64)
            .ok_or(AgentSessionError::Failed)?;
        let mut mode_choices = vec![json!({"value":"native","name":"Native settings"})];
        mode_choices.extend(
            config::modes()
                .into_iter()
                .map(|mode| json!({"value":mode["id"],"name":mode["label"]})),
        );
        let mut options = vec![
            json!({"id":"model","name":"Model","category":"model","type":"select","currentValue":runtime.model.unwrap_or_default(),"options":choices}),
            json!({"id":"thinking","name":"Thinking","category":"thought_level","type":"select","currentValue":runtime.thinking_option_id.unwrap_or_else(||"off".to_owned()),
                "options":levels.iter().map(|level|json!({"value":level,"name":level})).collect::<Vec<_>>()}),
            json!({"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":runtime.mode_id.unwrap_or_else(||"native".to_owned()),"options":mode_choices}),
        ];
        let controls = self
            .native()
            .and_then(|session| session.control_settings(&self.config))
            .ok_or(AgentSessionError::Failed)?;
        for feature in controls["features"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?
        {
            options.push(json!({"id":feature["id"],"name":feature["label"],"type":"select","currentValue":feature["value"].to_string(),
                "options":[{"value":"true","name":"Enabled"},{"value":"false","name":"Disabled"}]}));
        }
        Ok(json!(options))
    }

    fn notify(&mut self, method: &str, params: Value) {
        let mut message = json!({"jsonrpc":"2.0","method":method});
        message["params"] = params;
        self.outgoing.push(message);
    }

    /// Drain bounded native progress, callbacks and terminal responses without waiting.
    /// Returns native/protocol failures that cannot be associated with an active request.
    pub(super) fn poll(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        let Some(native) = &mut self.native else {
            return Ok(Vec::new());
        };
        if self.closed {
            return Ok(Vec::new());
        }
        let event = native.poll_turn();
        for frame in native.extension_frames() {
            if frame["type"] == "response"
                && let Some(id) = frame["id"].as_str()
                && let Some(id) = self.native_controls.remove(id)
            {
                if frame["success"] == true {
                    self.outgoing
                        .push(json!({"jsonrpc":"2.0","id":id,"result":frame["data"]}));
                } else {
                    self.outgoing
                        .push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,
                    "message":frame["error"].as_str().unwrap_or("OMP rejected native command")}}));
                }
            }
            if self.capabilities.raw_extensions {
                self.outgoing.push(json!({"jsonrpc":"2.0","method":"_omp/event","params":{"sessionId":self.id,"frame":frame}}));
            }
        }
        let event = match event {
            Ok(Some(event)) => event,
            Ok(None) => return Ok(std::mem::take(&mut self.outgoing)),
            Err(error) => {
                if self.active.is_none() {
                    return Err(error);
                }
                AgentTurnEvent::Failed
            }
        };
        if matches!(
            event,
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed
        ) {
            self.save_alias()?;
            if let Some((id, _)) = self.active.take() {
                let meta = if self.capabilities.ait {
                    json!({"ait":{"event":event}})
                } else {
                    json!({})
                };
                if event == AgentTurnEvent::Failed {
                    let message = self
                        .native()
                        .and_then(AgentSession::failure_message)
                        .map_or_else(
                            || "OMP execution failed".to_owned(),
                            std::borrow::Cow::into_owned,
                        );
                    let diagnostic = self
                        .native()
                        .and_then(AgentSession::failure_diagnostic)
                        .map(|facts| {
                            json!({"category":facts.category.code(),"nativeCode":facts.native_code,
                            "httpStatus":facts.http_status,"willRetry":facts.will_retry})
                        });
                    self.outgoing.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":message,"data":{"_meta":meta,"diagnostic":diagnostic}}}));
                } else {
                    self.outgoing.push(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":if event==AgentTurnEvent::Cancelled {"cancelled"} else {"end_turn"},"_meta":meta}}));
                }
            } else if self.capabilities.ait {
                self.notify("_ait/event", json!({"sessionId":self.id,"event":event}));
            }
        } else {
            if let AgentTurnEvent::PermissionRequested(request) = &event {
                if !self.capabilities.ait
                    && let Some(callback) =
                        mapping::question(&self.id, request, self.capabilities.elicitation)?
                {
                    let id = config::text(&callback, "id")?.to_owned();
                    self.callbacks.insert(id, request.clone());
                    self.outgoing.push(callback);
                } else if !self.capabilities.ait && !self.capabilities.raw_extensions {
                    if let Some((id, _)) = self.active.take() {
                        self.outgoing.push(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,
                            "message":"ACP client does not support the form elicitation required by this OMP extension"}}));
                    }
                    self.native.take();
                    self.closed = true;
                    return Ok(std::mem::take(&mut self.outgoing));
                }
            }
            if let AgentTurnEvent::PermissionResolved(id) = &event {
                self.callbacks.retain(|_, request| request["id"] != *id);
            }
            self.outgoing.extend(self.mapping.update(&self.id, &event)?);
            if self.capabilities.ait {
                self.notify("_ait/event", json!({"sessionId":self.id,"event":event}));
            }
        }
        Ok(std::mem::take(&mut self.outgoing))
    }

    fn save_alias(&self) -> Result<(), AgentSessionError> {
        if !self.capabilities.ait
            && let Some(handle) = self.native().and_then(AgentSession::persistence)
            && handle.session_id != self.id
        {
            aliases::save(
                &self.client,
                &self.id,
                &aliases::Alias {
                    cwd: self.cwd.clone(),
                    handle,
                    config: self.config.clone(),
                },
            )?;
        }
        Ok(())
    }

    /// Release the owned process, retaining history; returns native shutdown failures.
    pub(super) async fn close(&mut self) -> Result<(), AgentSessionError> {
        let saved = self.save_alias();
        let closed = if let Some(native) = &mut self.native {
            native.close().await
        } else {
            Ok(())
        };
        self.closed = true;
        saved.and(closed)
    }
}
