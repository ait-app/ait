use std::collections::BTreeMap;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    OmpClient, config, history, interactions::Dialogs, stream::Stream, transport::Transport,
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

#[derive(Debug)]
pub(super) struct Session {
    client: OmpClient,
    transport: Transport,
    cwd: String,
    state: Value,
    config: StoredAgentConfig,
    stream: Stream,
    dialogs: Dialogs,
    active: Option<String>,
    closed: bool,
    children: super::subagents::Children,
    prompt_id: Option<String>,
    // An earlier prompt's uncorrelated session_settled can follow the next admission.
    awaiting_settlement: bool,
    submitted: bool,
    created_at: String,
}

pub(super) async fn probe(client: &OmpClient, cwd: &str) -> Result<Transport, AgentSessionError> {
    config::validate_spec(&AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: cwd.to_owned(),
        config: StoredAgentConfig::default(),
    })?;
    let mut transport = Transport::spawn(
        client,
        cwd,
        &[
            "--mode".to_owned(),
            "rpc-ui".to_owned(),
            "--no-session".to_owned(),
        ],
    )?;
    transport.initialize().await?;
    transport.request("get_state", json!({})).await?;
    Ok(transport)
}

async fn connect(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    path: Option<&str>,
) -> Result<(Transport, Value), AgentSessionError> {
    let mut args = vec!["--mode".to_owned(), "rpc-ui".to_owned()];
    if let Some(path) = path {
        args.extend(["--resume".to_owned(), path.to_owned()]);
    }
    if let Some(prompt) = &spec.config.system_prompt {
        args.extend(["--append-system-prompt".to_owned(), prompt.clone()]);
    }
    if let Some(mode) = &spec.config.mode_id {
        args.extend([
            "--approval-mode".to_owned(),
            config::approval_mode(mode).to_owned(),
        ]);
    }
    let mut transport = Transport::spawn(client, &spec.cwd, &args)?;
    transport.initialize().await?;
    let state = config::apply(&mut transport, &spec.config).await?;
    transport
        .request("set_ask_dialog", json!({"enabled":true}))
        .await?;
    transport
        .request("set_subagent_subscription", json!({"level":"events"}))
        .await?;
    config::runtime(&state)?;
    Ok((transport, state))
}

pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    let journal = if let Some(handle) = handle {
        Some(history::load(client, handle, &spec.cwd).await?)
    } else {
        None
    };
    let path = journal
        .as_ref()
        .map(|journal| journal.path.to_str().ok_or(AgentSessionError::Failed))
        .transpose()?;
    let persisted = path.is_some_and(|path| std::path::Path::new(path).is_file());
    let created_at = journal
        .as_ref()
        .and_then(|journal| journal.header["timestamp"].as_str())
        .map_or_else(|| chrono::Utc::now().to_rfc3339(), str::to_owned);
    let (transport, state) = connect(client, spec, path.filter(|_| persisted)).await?;
    let id = config::text(&state, "sessionId")?;
    if persisted && handle.is_some_and(|handle| handle.session_id != id) {
        return Err(AgentSessionError::Failed);
    }
    let mut stream = match (journal, handle) {
        (Some(journal), Some(handle)) if persisted => journal.project(client, handle)?.1,
        _ => Stream::new(id.to_owned(), client.images.clone()),
    };
    stream.set_context(&state);
    Ok(Session {
        client: client.clone(),
        transport,
        cwd: spec.cwd.clone(),
        state,
        config: spec.config.clone(),
        stream,
        dialogs: Dialogs::default(),
        active: None,
        closed: false,
        children: super::subagents::Children::default(),
        prompt_id: None,
        awaiting_settlement: false,
        submitted: false,
        created_at,
    })
}

impl Session {
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
        if self.config != *config {
            self.transport.close().await?;
            let path = config::text(&self.state, "sessionFile")?.to_owned();
            let spec = AgentSessionSpec {
                provider: "omp".to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
            };
            let persisted = std::path::Path::new(&path).is_file();
            let (transport, state) =
                connect(&self.client, &spec, persisted.then_some(path.as_str())).await?;
            if persisted && state["sessionId"] != self.state["sessionId"] {
                return Err(AgentSessionError::Failed);
            }
            if !persisted {
                self.stream = Stream::new(
                    config::text(&state, "sessionId")?.to_owned(),
                    self.client.images.clone(),
                );
            }
            self.transport = transport;
            self.state = state;
            self.stream.set_context(&self.state);
            self.config.clone_from(config);
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
        self.stream
            .pending_inputs
            .push_back(prompt.client_message_id.clone());
        let response = self
            .transport
            .request("prompt", json!({"message":text,"images":images}))
            .await;
        self.prompt_id = Some(self.transport.last_request_id());
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                self.stream.pending_inputs.pop_back();
                return Err(error);
            }
        };
        let turn = Uuid::new_v4().to_string();
        self.submitted |= response["agentInvoked"] != false;
        self.stream.begin(turn.clone());
        self.active = Some(turn.clone());
        self.awaiting_settlement = false;
        if response["agentInvoked"] == false {
            self.stream.pending_inputs.pop_back();
            self.active = None;
            self.prompt_id = None;
            self.stream
                .events
                .push_back(AgentTurnEvent::Completed(None));
        }
        Ok(turn)
    }

    fn consume(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        match event["type"].as_str() {
            Some("extension_ui_request") => self.stream.events.extend(self.dialogs.capture(event)?),
            Some("agent_start") if self.active.is_none() => {
                let turn = Uuid::new_v4().to_string();
                self.stream.begin(turn.clone());
                self.active = Some(turn.clone());
                self.awaiting_settlement = true;
                self.stream.events.push_back(AgentTurnEvent::Started(turn));
            }
            Some("subagent_lifecycle" | "subagent_progress" | "subagent_event") => {
                self.stream.events.extend(self.children.consume(
                    event,
                    config::text(&self.state, "sessionId")?,
                    &self.cwd,
                    &self.client.images,
                )?);
            }
            Some("prompt_result") if self.prompt_id.as_deref() == event["id"].as_str() => {
                self.stream.failed = event["status"] == "error";
                self.stream.aborted = event["status"] == "aborted";
                self.awaiting_settlement = true;
                if event["sessionSettled"] == true {
                    self.finish();
                }
            }
            Some("session_settled") if self.awaiting_settlement => self.finish(),
            Some("rpc_frame_error" | "host_tool_call" | "host_uri_request") => {
                return Err(AgentSessionError::Failed);
            }
            Some("response") if event["success"] == false => return Err(AgentSessionError::Failed),
            _ => self.stream.consume(event)?,
        }
        Ok(())
    }

    fn finish(&mut self) {
        if self.active.take().is_some() {
            self.prompt_id = None;
            self.awaiting_settlement = false;
            self.stream.events.extend(self.dialogs.clear());
            self.stream.events.push_back(if self.stream.aborted {
                AgentTurnEvent::Cancelled
            } else if self.stream.failed {
                AgentTurnEvent::Failed
            } else {
                AgentTurnEvent::Completed(self.stream.last_text.clone())
            });
        }
    }
}

impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "omp"
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async {
            let state = self.transport.request("get_state", json!({})).await?;
            if state["sessionId"] != self.state["sessionId"] {
                return Err(AgentSessionError::Failed);
            }
            self.state = state;
            self.stream.set_context(&self.state);
            let mut info = config::runtime(&self.state)?;
            info.mode_id.clone_from(&self.config.mode_id);
            Ok(info)
        })
    }
    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.children.list()
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let file = self.state["sessionFile"].as_str()?;
        Some(AgentPersistenceHandle {
            provider: "omp".to_owned(),
            session_id: self.state["sessionId"].as_str()?.to_owned(),
            native_handle: None,
            metadata: Some(BTreeMap::from([
                ("cwd".to_owned(), json!(self.cwd)),
                ("sessionFile".to_owned(), json!(file)),
                ("inputIds".to_owned(), json!(self.stream.input_ids)),
                (
                    "empty".to_owned(),
                    json!(!self.submitted && self.stream.is_empty()),
                ),
                ("createdAt".to_owned(), json!(self.created_at)),
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
    fn steer_input<'a>(
        &'a mut self,
        turn: &'a str,
        prompt: &'a AgentPrompt,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            prompt.validate()?;
            if self.closed || self.active.as_deref() != Some(turn) || prompt.output_schema.is_some()
            {
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
            self.transport
                .request("steer", json!({"message":text,"images":images}))
                .await?;
            self.stream
                .pending_inputs
                .push_back(prompt.client_message_id.clone());
            Ok(())
        })
    }
    fn steer_turn<'a>(&'a mut self, turn: &'a str, text: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move { self.steer_input(turn, &AgentPrompt::text(text)).await })
    }
    fn cancel_turn<'a>(&'a mut self, turn: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.active.as_deref() != Some(turn) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            self.transport
                .request("abort_and_restore_queue", json!({}))
                .await?;
            self.stream.aborted = true;
            self.stream.pending_inputs.clear();
            Ok(())
        })
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.dialogs.pending()
    }
    fn respond_permission<'a>(
        &'a mut self,
        id: &'a str,
        response: &'a Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            let native = self.dialogs.response(id, response)?;
            self.transport.send(&native).await?;
            self.dialogs.remove(id);
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
        for _ in 0..64 {
            let Some(event) = self.transport.poll()? else {
                return Ok(None);
            };
            self.consume(&event)?;
            if let Some(event) = self.stream.events.pop_front() {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            self.closed = true;
            self.stream.events.extend(self.dialogs.clear());
            self.transport.close().await
        })
    }
}
