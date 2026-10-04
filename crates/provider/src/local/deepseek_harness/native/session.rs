//! Coordinate one native interactive session through existing Ait session ports.
use super::super::{DeepSeekHarnessClient, PROVIDER, config::text, streaming::Stream};
use super::{
    config::{self, Selection},
    interactions::Pending,
    runtime::Runtime,
};
use crate::{
    ports::agent_session::{
        AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
    },
    protocol::{prompt::AgentPrompt, provider::Details},
};
use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::task::JoinSet;

#[derive(Debug)]
pub(in crate::local::deepseek_harness) struct Session {
    runtime: Runtime,
    id: String,
    cwd: String,
    selection: Selection,
    stream: Stream,
    cursor: i64,
    active: Option<String>,
    native_turn: Option<u64>,
    permissions: BTreeMap<String, Pending>,
    tools: BTreeMap<String, Value>,
    waiting_approvals: BTreeMap<String, Value>,
    delegations: JoinSet<Result<Value, AgentSessionError>>,
    closed: bool,
    images: crate::local::images::ImageStore,
    hydration: JoinSet<Result<Value, AgentSessionError>>,
}

/// Open or adopt one durable native session and verify its cwd before admitting work.
pub(in crate::local::deepseek_harness) async fn open(
    client: &DeepSeekHarnessClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate(&spec.config)?;
    if spec.provider != PROVIDER
        || !std::path::Path::new(&spec.cwd).is_absolute()
        || !std::path::Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(handle) = handle
        && (handle.provider != PROVIDER
            || handle.session_id.is_empty()
            || handle.session_id.len() > 1024
            || handle
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cwd"))
                .and_then(Value::as_str)
                != Some(&spec.cwd))
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut runtime = Runtime::open(client, &spec.cwd).await?;
    let catalog = runtime.api.call("session/modelCatalog", json!({})).await?;
    let mut request = json!({"cwd":spec.cwd});
    if let Some(handle) = handle {
        request["sessionId"] = json!(handle.session_id);
    }
    let created = runtime
        .api
        .call("session/create", json!({"request":request}))
        .await?;
    let id = text(&created, "sessionId")?.to_owned();
    if handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    runtime
        .subscribe(
            "history",
            "session/follow",
            json!({"request":{"address":{"kind":"session","sessionId":id},"maxMessages":1}}),
        )
        .await?;
    let snapshot = loop {
        let frame = runtime.next().await?;
        if frame["streamId"] == "history" {
            if frame["type"] != "item" || frame["value"]["type"] != "snapshot" {
                return Err(AgentSessionError::Failed);
            }
            break frame["value"].clone();
        }
        if frame["streamId"] != "events"
            || frame["type"] != "item"
            || frame["value"]["type"] != "emit"
        {
            return Err(AgentSessionError::Failed);
        }
    };
    if snapshot["header"]["id"] != id
        || snapshot["header"]["cwd"] != spec.cwd
        || snapshot["header"]["origin"] == "subagent"
    {
        return Err(AgentSessionError::Failed);
    }
    let values = &snapshot["projections"]["values"];
    let selected = values["modelSelection"]["next"].as_object().map_or_else(
        || catalog["default"].clone(),
        |value| Value::Object(value.clone()),
    );
    let mut selection = Selection {
        catalog,
        permissions: values["permissions"].clone(),
        model: selected,
    };
    selection.apply(&runtime.api, &id, &spec.config).await?;
    runtime
        .subscribe("control", "session/control", json!({}))
        .await?;
    Ok(Session {
        runtime,
        id,
        cwd: spec.cwd.clone(),
        selection,
        stream: Stream::new(client.images.clone()),
        cursor: snapshot["cursor"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?,
        active: None,
        native_turn: None,
        permissions: BTreeMap::new(),
        tools: BTreeMap::new(),
        waiting_approvals: BTreeMap::new(),
        delegations: JoinSet::new(),
        closed: false,
        images: client.images.clone(),
        hydration: JoinSet::new(),
    })
}

impl Session {
    pub(in crate::local::deepseek_harness) fn details(&self) -> Result<Details, AgentSessionError> {
        self.selection.details()
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        if self.closed
            || self.active.is_some()
            || !self.permissions.is_empty()
            || prompt.output_schema.is_some()
        {
            return Err(AgentSessionError::Rejected);
        }
        let content = super::content::prompt(prompt)?;
        self.selection
            .apply(&self.runtime.api, &self.id, config)
            .await?;
        let turn = uuid::Uuid::new_v4().to_string();
        self.stream.begin(turn.clone());
        self.tools.clear();
        self.native_turn = None;
        self.active = Some(turn.clone());
        let result=self.runtime.api.call("session/prompt",json!({"request":{"sessionId":self.id,"requestId":turn,"mode":"queue","content":content}})).await;
        if result.as_ref().is_err() || result.as_ref().is_ok_and(|value| value["accepted"] != true)
        {
            // Admission may have succeeded before transport failure. Never resend this input.
            let _ = self.runtime.close().await;
            self.closed = true;
            return Err(AgentSessionError::Failed);
        }
        self.stream.events.push_back(AgentTurnEvent::RuntimeInfo(
            self.selection.runtime(&self.id),
        ));
        Ok(turn)
    }

    fn consume(&mut self, frame: &Value) -> Result<(), AgentSessionError> {
        if frame["type"] != "item" {
            return Err(AgentSessionError::Failed);
        }
        match frame["streamId"].as_str() {
            Some("events") => self.interaction(&frame["value"]),
            Some("control") => {
                let frame = &frame["value"];
                let value = if frame["type"] == "baseline" {
                    &frame["value"]["projections"][&self.id]["values"]["contextPressure"]
                } else if frame["type"] == "projection"
                    && frame["sessionId"] == self.id
                    && frame["key"] == "contextPressure"
                {
                    &frame["value"]
                } else {
                    return Ok(());
                };
                if let Some(usage) = super::usage::context(value)? {
                    self.stream.events.push_back(AgentTurnEvent::Usage(usage));
                }
                Ok(())
            }
            Some("history") => {
                let value = &frame["value"];
                if value["type"] != "event" {
                    return Err(AgentSessionError::Failed);
                }
                let event = &value["event"];
                let seq = event["seq"].as_i64().ok_or(AgentSessionError::Failed)?;
                if seq != self.cursor + 1 {
                    return Err(AgentSessionError::Failed);
                }
                self.cursor = seq;
                if self.active.is_some() {
                    self.history(event)?;
                }
                Ok(())
            }
            _ => Err(AgentSessionError::Failed),
        }
    }

    fn interaction(&mut self, frame: &Value) -> Result<(), AgentSessionError> {
        match frame["type"].as_str() {
            Some("emit") => Ok(()),
            Some("cancel") => {
                if let Some(id) = frame["eventId"].as_str() {
                    self.waiting_approvals.remove(id);
                }
                if let Some(id) = self
                    .permissions
                    .iter()
                    .find(|(_, pending)| frame["eventId"] == pending.event_id)
                    .map(|(id, _)| id.clone())
                {
                    self.permissions.remove(&id);
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::PermissionResolved(id));
                }
                Ok(())
            }
            Some("waterfall") => {
                if frame["agentId"] != self.id
                    || !matches!(
                        frame["event"].as_str(),
                        Some("user-questions/request" | "approval/request")
                    )
                {
                    let api = self.runtime.api.clone();
                    let args = json!({"clientId":self.runtime.client_id,"eventId":text(frame,"eventId")?,"outcome":{"kind":"next"}});
                    if self.delegations.len() >= 128 {
                        return Err(AgentSessionError::Failed);
                    }
                    self.delegations
                        .spawn(async move { api.call("$events/result", args).await });
                    return Ok(());
                }
                if self.active.is_none()
                    || self.permissions.len() + self.waiting_approvals.len() >= 128
                    || self.waiting_approvals.contains_key(text(frame, "eventId")?)
                    || self
                        .permissions
                        .values()
                        .any(|pending| frame["eventId"] == pending.event_id)
                {
                    return Err(AgentSessionError::Failed);
                }
                let tool = frame["request"]["callId"]
                    .as_str()
                    .and_then(|id| self.tools.get(id));
                if frame["event"] == "approval/request"
                    && frame["request"]["callId"].is_string()
                    && tool.is_none()
                {
                    self.waiting_approvals
                        .insert(text(frame, "eventId")?.into(), frame.clone());
                    return Ok(());
                }
                let pending = Pending::capture(frame, tool)?;
                let id = text(&pending.request, "id")?.to_owned();
                self.stream
                    .events
                    .push_back(AgentTurnEvent::PermissionRequested(pending.request.clone()));
                self.permissions.insert(id, pending);
                Ok(())
            }
            _ => Err(AgentSessionError::Failed),
        }
    }

    fn history(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        let data = &event["data"];
        match event["type"].as_str() {
            Some("turn/start") => {
                if self.native_turn.is_some() {
                    return Err(AgentSessionError::Failed);
                }
                self.native_turn = Some(data["turn"].as_u64().ok_or(AgentSessionError::Failed)?);
            }
            Some("assistant/message") => {
                let message = &data["message"];
                for block in message["content"]
                    .as_array()
                    .ok_or(AgentSessionError::Failed)?
                {
                    match block["type"].as_str(){
                        Some("text"|"reasoning")=>self.stream.update(&json!({"sessionUpdate":if block["type"]=="text"{"agent_message_chunk"}else{"agent_thought_chunk"},"messageId":message["id"],"content":{"type":"text","text":block["text"]}}))?,
                        Some("tool-call")=>{},
                        _=>return Err(AgentSessionError::Failed),
                    }
                }
                self.stream.flush();
            }
            Some("tool/call") => {
                let id = text(data, "callId")?;
                let input: Value = serde_json::from_str(
                    data["arguments"]
                        .as_str()
                        .ok_or(AgentSessionError::Failed)?,
                )
                .map_err(|_| AgentSessionError::Failed)?;
                if self.tools.len() >= 4096
                    || self
                        .tools
                        .insert(id.into(), json!({"name":text(data,"name")?,"input":input}))
                        .is_some()
                {
                    return Err(AgentSessionError::Failed);
                }
                self.stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":id,"title":data["name"],"status":"in_progress","rawInput":input}))?;
                let ready: Vec<_> = self
                    .waiting_approvals
                    .iter()
                    .filter(|(_, frame)| frame["request"]["callId"] == id)
                    .map(|(key, _)| key.clone())
                    .collect();
                for key in ready {
                    if let Some(frame) = self.waiting_approvals.remove(&key) {
                        self.interaction(&frame)?;
                    }
                }
            }
            Some("tool/result") => {
                let block = &data["message"]["content"][0];
                let id = text(block, "toolCallId")?;
                if !self.tools.contains_key(id) {
                    return Err(AgentSessionError::Failed);
                }
                self.stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":id,"status":if block["isError"]==true{"failed"}else{"completed"},"rawOutput":block["content"]}))?;
            }
            Some("turn/end") => {
                if data["turn"].as_u64() != self.native_turn || self.native_turn.is_none() {
                    return Err(AgentSessionError::Failed);
                }
                self.stream.flush();
                self.waiting_approvals.clear();
                for id in std::mem::take(&mut self.permissions).into_keys() {
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::PermissionResolved(id));
                }
                self.stream
                    .events
                    .push_back(match data["reason"]["kind"].as_str() {
                        Some("completed" | "max-tokens") => {
                            AgentTurnEvent::Completed(self.stream.last_message.clone())
                        }
                        Some("aborted" | "interrupted") => AgentTurnEvent::Cancelled,
                        _ => AgentTurnEvent::Failed,
                    });
                self.active = None;
                self.native_turn = None;
            }
            _ => {}
        }
        Ok(())
    }
}

impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Ok(self.selection.runtime(&self.id)) })
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: PROVIDER.into(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([
                ("cwd".into(), json!(self.cwd)),
                ("transport".into(), json!("native-host")),
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
        if let Some(event) = self.stream.events.pop_front() {
            return Ok(Some(event));
        }
        while let Some(result) = self.delegations.try_join_next() {
            result.map_err(|_| AgentSessionError::Failed)??;
        }
        if !self.hydration.is_empty() {
            if let Some(result) = self.hydration.try_join_next() {
                self.consume(&result.map_err(|_| AgentSessionError::Failed)??)?;
                return Ok(self.stream.events.pop_front());
            }
            return Ok(None);
        }
        for _ in 0..128 {
            let Some(frame) = self.runtime.poll()? else {
                return Ok(None);
            };
            if super::content::has_images(&frame) {
                let api = self.runtime.api.clone();
                let id = self.id.clone();
                let images = self.images.clone();
                self.hydration
                    .spawn(async move { super::content::hydrate(frame, &api, &id, &images).await });
                return Ok(None);
            }
            self.consume(&frame)?;
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
            let outcome = pending.resolve(response)?;
            self.runtime.api.call("$events/result",json!({"clientId":self.runtime.client_id,"eventId":pending.event_id,"outcome":outcome})).await?;
            self.permissions.remove(id);
            self.stream
                .events
                .push_back(AgentTurnEvent::PermissionResolved(id.into()));
            if response["behavior"] == "deny" && response["interrupt"] == true {
                self.runtime
                    .api
                    .call("session/cancel", json!({"request":{"sessionId":self.id}}))
                    .await?;
            }
            Ok(())
        })
    }
    fn cancel_turn<'a>(&'a mut self, id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.active.as_deref() != Some(id) {
                return Err(AgentSessionError::Rejected);
            }
            self.runtime
                .api
                .call("session/cancel", json!({"request":{"sessionId":self.id}}))
                .await?;
            Ok(())
        })
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            if self.closed {
                return Ok(());
            }
            self.closed = true;
            self.delegations.abort_all();
            self.hydration.abort_all();
            self.runtime.close().await
        })
    }
}
