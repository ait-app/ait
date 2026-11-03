//! Coordinate one native interactive session through existing Ait session ports.
use crate::ports::agent_session::AgentSessionFuture;
use std::borrow::Cow;
use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::super::{DeepSeekHarnessClient, PROVIDER, config::text};
use super::{
    config::{self, Selection},
    interactions::Pending,
    presets,
    runtime::Runtime,
};
use crate::local::acp_streaming::Stream;
use crate::{
    ports::agent_session::{AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent},
    protocol::{prompt::AgentPrompt, provider::Details},
};

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
    projection: super::projection::State,
    waiting_approvals: BTreeMap<String, Value>,
    delegations: JoinSet<Result<Value, AgentSessionError>>,
    closed: bool,
    images: crate::local::images::ImageStore,
    hydration: JoinSet<Result<Value, AgentSessionError>>,
    account: Result<super::quota::credentials::Context, AgentSessionError>,
    commands: super::commands::State,
    command_completion: Option<tokio::sync::oneshot::Receiver<Result<(), AgentSessionError>>>,
    exports: super::exports::Store,
    exporting: bool,
    export_result: Option<crate::protocol::timeline::NativeItem>,
}

/// Open or adopt one durable native session and verify its cwd before admitting work.
pub(in crate::local::deepseek_harness) async fn open(
    client: &DeepSeekHarnessClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    validate_identity(spec, handle)?;
    let runtime = Runtime::open(client, &spec.cwd).await?;
    open_runtime(client, spec, handle, runtime).await
}

/// Discover the receiving Agent's command composition without retaining a native conversation.
/// Returns native configuration, profile-overlay, and transport failures without model input.
pub(in crate::local::deepseek_harness) async fn preview(
    client: &DeepSeekHarnessClient,
    spec: &AgentSessionSpec,
) -> Result<Session, AgentSessionError> {
    validate_identity(spec, None)?;
    let runtime = Runtime::preview(client, &spec.cwd).await?;
    open_runtime(client, spec, None, runtime).await
}

async fn open_runtime(
    client: &DeepSeekHarnessClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
    mut runtime: Runtime,
) -> Result<Session, AgentSessionError> {
    let account = super::quota::credentials::Context::capture(client, &spec.cwd).await;
    let catalog = runtime.api.call("session/modelCatalog", json!({})).await?;
    let presets = runtime.api.call("agentPresets/list", json!({})).await?;
    let mut request = json!({"cwd":spec.cwd});
    if let Some(mode) = spec
        .config
        .mode_id
        .as_deref()
        .filter(|id| id.starts_with(presets::PREFIX))
    {
        if handle.is_none()
            && !presets::modes(&presets)?
                .iter()
                .any(|option| option["id"] == mode)
        {
            return Err(AgentSessionError::Rejected);
        }
        request["agentPreset"] = json!(mode.strip_prefix(presets::PREFIX));
    }
    let id = if let Some(handle) = handle {
        super::history::validate_root(&runtime, &handle.session_id, &spec.cwd).await?;
        handle.session_id.clone()
    } else {
        let created = runtime
            .api
            .call("session/create", json!({"request":request}))
            .await?;
        text(&created, "sessionId")?.to_owned()
    };
    let snapshot = follow_snapshot(&mut runtime, &id, &spec.cwd).await?;
    let values = &snapshot["projections"]["values"];
    let selected = values["modelSelection"]["next"].as_object().map_or_else(
        || catalog["default"].clone(),
        |value| Value::Object(value.clone()),
    );
    let mut selection = Selection {
        presets,
        preset: values["agentPreset"].as_str().map(str::to_owned),
        catalog,
        permissions: config::permission_selection(&runtime.api, &values["permissions"]).await?,
        model: selected,
    };
    selection
        .apply(&runtime.api, &id, &spec.config, handle.is_some())
        .await?;
    runtime
        .subscribe("control", "session/control", json!({}))
        .await?;
    let commands = super::commands::State::open(&runtime.api, &id).await?;
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
        projection: super::projection::State::from_projection(values)?,
        waiting_approvals: BTreeMap::new(),
        delegations: JoinSet::new(),
        closed: false,
        images: client.images.clone(),
        hydration: JoinSet::new(),
        account,
        commands,
        command_completion: None,
        exports: client.exports.clone(),
        exporting: false,
        export_result: None,
    })
}

async fn follow_snapshot(
    runtime: &mut Runtime,
    id: &str,
    cwd: &str,
) -> Result<Value, AgentSessionError> {
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
        || snapshot["header"]["cwd"] != cwd
        || snapshot["header"]["origin"] == "subagent"
    {
        return Err(AgentSessionError::Failed);
    }
    Ok(snapshot)
}

fn validate_identity(
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<(), AgentSessionError> {
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
    Ok(())
}

impl Session {
    async fn execute_command(&mut self, prompt: &AgentPrompt) -> Result<(), AgentSessionError> {
        prompt.validate()?;
        let name = super::commands::name(&prompt.text)
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        if self.closed || self.commands.catalog.is_none() || prompt.output_schema.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        if !self.commands.runs.is_empty() {
            return Err(AgentSessionError::Busy);
        }
        let has_attachments = !prompt.images.is_empty() || !prompt.attachments.is_empty();
        let export = self.commands.catalog.as_ref().is_some_and(|catalog| {
            catalog.iter().any(|command| {
                command["name"] == name
                    && command["definitionId"] == "@deepseek-ai/dsh-session-log-export"
            })
        });
        if has_attachments || export {
            if has_attachments
                && !self.commands.catalog.as_ref().is_some_and(|catalog| {
                    catalog.iter().any(|command| {
                        command["name"] == name && command["acceptsAttachments"] == true
                    })
                })
            {
                return Err(AgentSessionError::Rejected);
            }
            let attachments = super::commands::attachments::prepare(prompt).await?;
            let api = self.runtime.api.control_scope();
            let id = self.id.clone();
            let line = prompt.text.clone();
            let exports = self.exports.clone();
            self.exporting = export;
            self.export_result = None;
            let (completed, completion) = tokio::sync::oneshot::channel();
            self.command_completion = Some(completion);
            self.commands.runs.spawn(async move {
                let result = async {
                    let submitted =
                        super::commands::attachments::submit(&api, &id, attachments).await?;
                    let native = api
                        .call(
                            "commands/execute",
                            json!({"agentId":id,"line":line,
                        "submittedAttachments":submitted}),
                        )
                        .await?;
                    super::commands::acknowledged(&native)?;
                    let exported = export && native["result"]["kind"] == "success";
                    if exported {
                        exports
                            .download(&api, &id, text(&native, "commandId")?)
                            .await?;
                    }
                    Ok(super::commands::Outcome { native, exported })
                }
                .await;
                let settled = result.as_ref().map_err(|error| *error).and_then(|outcome| {
                    if outcome.native["result"]["kind"] == "error" {
                        return Err(AgentSessionError::Rejected);
                    }
                    Ok(())
                });
                let _ = completed.send(settled);
                result
            });
            return Ok(());
        }
        let api = self.runtime.api.control_scope();
        let args = json!({"agentId":self.id,"line":prompt.text,"submittedAttachments":[]});
        self.commands.runs.spawn(async move {
            api.call("commands/execute", args)
                .await
                .map(|native| super::commands::Outcome {
                    native,
                    exported: false,
                })
        });
        loop {
            tokio::select! {
                result = self.commands.runs.join_next() => {
                    let value = result.ok_or(AgentSessionError::Failed)?.map_err(|_|AgentSessionError::Failed)??;
                    return super::commands::acknowledged(&value.native);
                }
                frame = self.runtime.next() => {
                    let mut frame = frame?;
                    let admitted = frame["streamId"] == "history"
                        && frame["value"]["event"]["type"] == "command/run"
                        && frame["value"]["event"]["data"]["name"] == name;
                    if super::content::has_images(&frame) {
                        frame = super::content::hydrate(frame, &self.runtime.api, &self.id, &self.images).await?;
                    }
                    self.consume(&frame)?;
                    if admitted { return Ok(()); }
                }
            }
        }
    }

    fn poll_commands(&mut self) {
        while let Some(result) = self.commands.runs.try_join_next() {
            let acknowledged = result
                .map_err(|_| AgentSessionError::Failed)
                .and_then(std::convert::identity)
                .and_then(|outcome| {
                    if outcome.exported {
                        self.exporting = false;
                        if let Some(mut entry) = self.export_result.take() {
                            self.exports.decorate(&mut entry)?;
                            self.stream
                                .events
                                .push_back(AgentTurnEvent::Timeline(entry));
                        }
                    }
                    super::commands::acknowledged(&outcome.native)
                });
            if acknowledged.is_err() {
                self.exporting = false;
                self.export_result = None;
                self.stream.events.push_back(AgentTurnEvent::Timeline(
                    crate::protocol::timeline::NativeItem {
                        key: format!("host:dsh:command-response:{}", uuid::Uuid::new_v4()),
                        turn_id: None,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        item: json!({"type":"error","message":
                            "DSH command response could not be read. Check the command result before retrying."}),
                    },
                ));
            }
        }
        self.commands.poll_refresh();
        if self.commands.dirty && self.commands.refresh.is_empty() {
            self.commands.dirty = false;
            let api = self.runtime.api.control_scope();
            let id = self.id.clone();
            self.commands
                .refresh
                .spawn(async move { super::commands::catalog(&api, &id).await });
        }
    }

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
        self.runtime.diagnostics.clear();
        let content = super::content::prompt(prompt)?;
        self.selection
            .apply(&self.runtime.api, &self.id, config, false)
            .await?;
        let turn = uuid::Uuid::new_v4().to_string();
        let request_id = prompt.client_message_id.as_deref().unwrap_or(&turn);
        self.stream.begin(turn.clone());
        self.projection.tools.clear();
        self.native_turn = None;
        self.active = Some(turn.clone());
        let result=self.runtime.api.call("session/prompt",json!({"request":{"sessionId":self.id,"requestId":request_id,"mode":"queue","content":content}})).await;
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
                if self.active.is_some()
                    || matches!(
                        event["type"].as_str(),
                        Some(
                            "user/message"
                                | "permission/preset"
                                | "goal/change"
                                | "turn/start"
                                | "command/run"
                                | "command/done"
                                | "compaction/start"
                                | "compaction/end"
                        )
                    )
                {
                    self.history(event)?;
                }
                Ok(())
            }
            _ => Err(AgentSessionError::Failed),
        }
    }

    fn interaction(&mut self, frame: &Value) -> Result<(), AgentSessionError> {
        match frame["type"].as_str() {
            Some("emit") => {
                if frame["event"] == "commands/change" {
                    self.commands.dirty = true;
                }
                Ok(())
            }
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
                if (self.active.is_none() && self.commands.runs.is_empty())
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
                    .and_then(|id| self.projection.tools.get(id));
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
        let before = self.stream.events.len();
        super::projection::apply(&mut self.stream, &mut self.projection, event, &self.id)?;
        for entry in self.stream.events.iter_mut().skip(before) {
            if let AgentTurnEvent::Timeline(entry) = entry {
                self.exports.decorate(entry)?;
                if self.exporting
                    && entry.item["type"] == "provider_command"
                    && entry.item["command"] == "export"
                {
                    self.export_result = Some(entry.clone());
                }
            }
        }
        let data = &event["data"];
        match event["type"].as_str() {
            Some("permission/preset") => {
                let value = text(data, "preset")?;
                self.selection.permissions["currentValue"] = json!(value);
                if value != "custom" {
                    self.stream
                        .events
                        .push_back(AgentTurnEvent::FeatureValuesUpdated(BTreeMap::from([(
                            presets::PERMISSION.to_owned(),
                            json!(value),
                        )])));
                }
            }
            Some("turn/start") => {
                if self.native_turn.is_some() {
                    return Err(AgentSessionError::Failed);
                }
                self.native_turn = Some(data["turn"].as_u64().ok_or(AgentSessionError::Failed)?);
                if self.active.is_none() {
                    let turn = format!("dsh:{}:{}", self.id, data["turn"]);
                    self.active = Some(turn.clone());
                    self.stream.events.push_back(AgentTurnEvent::Started(turn));
                }
            }
            Some("tool/call") => {
                let id = text(data, "callId")?;
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
                if !matches!(
                    data["reason"]["kind"].as_str(),
                    Some("completed" | "max-tokens" | "aborted" | "interrupted")
                ) {
                    self.runtime
                        .diagnostics
                        .observe_native(&data["reason"], "result", "turn");
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

#[async_trait::async_trait]
impl AgentSession for Session {
    fn commands(&self) -> Option<&[Value]> {
        self.commands.catalog.as_deref()
    }

    async fn refresh_commands(&mut self) -> Result<Option<Vec<Value>>, AgentSessionError> {
        let api = self.runtime.api.control_scope();
        self.commands.catalog = super::commands::catalog(&api, &self.id).await?;
        Ok(self.commands.catalog.clone())
    }

    async fn out_of_band(&mut self, prompt: &AgentPrompt) -> Result<(), AgentSessionError> {
        self.execute_command(prompt).await
    }

    fn take_command_completion(&mut self) -> Option<AgentSessionFuture<'static, ()>> {
        let completion = self.command_completion.take()?;
        Some(Box::pin(async move {
            completion.await.map_err(|_| AgentSessionError::Failed)?
        }))
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        config::validate(config)?;
        self.selection.validate_controls(config).map(|_| ())
    }

    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        let mut details = self.selection.details().ok()?;
        // A composed session exposes its identity, not choices that would replace its plugins.
        let mode = self.selection.runtime(&self.id).mode_id;
        details
            .modes
            .retain(|option| option["id"].as_str() == mode.as_deref());
        if let Some(value) = config
            .feature_values
            .as_ref()
            .and_then(|values| values.get(presets::PERMISSION))
            && let Some(feature) = details.features.first_mut()
        {
            feature["value"] = value.clone();
        }
        Some(json!({"availableModes":details.modes,"features":details.features}))
    }

    async fn account_usage(&self) -> Result<Value, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Unavailable);
        }
        super::quota::query(&self.runtime.api, &self.account).await
    }

    async fn prepare_account_usage(
        &self,
    ) -> Result<crate::ports::account_usage::PreparedUsage<'_>, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Unavailable);
        }
        super::quota::prepare(&self.runtime.api, &self.account).await
    }

    fn failure_diagnostic(&self) -> Option<crate::ports::diagnostics::Diagnostic> {
        self.runtime.diagnostics.failure()
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.runtime.diagnostics.message().map(Cow::Owned)
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(self.selection.runtime(&self.id))
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
        self.poll_commands();
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
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
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
    }
    async fn cancel_turn(&mut self, id: &str) -> Result<(), AgentSessionError> {
        if self.active.as_deref() != Some(id) {
            return Err(AgentSessionError::Rejected);
        }
        self.runtime
            .api
            .call("session/cancel", json!({"request":{"sessionId":self.id}}))
            .await?;
        Ok(())
    }
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.delegations.abort_all();
        self.hydration.abort_all();
        self.commands.runs.abort_all();
        self.commands.refresh.abort_all();
        self.runtime.close().await
    }
}
