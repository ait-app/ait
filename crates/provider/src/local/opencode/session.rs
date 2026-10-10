//! One prompt request, bidirectional ACP callbacks and native replay before writer release.
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::task::AbortOnDropHandle;

use super::{OpenCodeClient, PROVIDER, config, history, interactions, launcher, streaming::Stream};
use crate::{
    local::acp_transport::{self, Transport},
    ports::agent_session::{
        AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec,
        AgentTurnEvent,
    },
    protocol::prompt::AgentPrompt,
};

type Pending = Arc<Mutex<BTreeMap<String, interactions::Pending>>>;

#[derive(Debug)]
struct Connection {
    transport: Transport,
    options: Value,
    capabilities: Value,
    clients: BTreeMap<String, String>,
    users: BTreeSet<String>,
}

#[derive(Debug)]
struct Finished {
    connection: Connection,
    event: AgentTurnEvent,
    failure: Option<String>,
}

#[derive(Debug)]
enum Command {
    Reply(
        String,
        Value,
        oneshot::Sender<Result<(), AgentSessionError>>,
    ),
    Cancel(oneshot::Sender<Result<(), AgentSessionError>>),
}

/// Owns one native identity and serializes prompt admission, callbacks and settlement.
#[derive(Debug)]
pub(super) struct Session {
    client: OpenCodeClient,
    spec: AgentSessionSpec,
    id: String,
    connection: Option<Connection>,
    info: StoredAgentRuntimeInfo,
    clients: BTreeMap<String, String>,
    pending: Pending,
    commands: Option<mpsc::Sender<Command>>,
    events: mpsc::Receiver<AgentTurnEvent>,
    finished: Option<oneshot::Receiver<Result<Finished, AgentSessionError>>>,
    task: Option<AbortOnDropHandle<()>>,
    queued: VecDeque<AgentTurnEvent>,
    turn: Option<String>,
    history_only: bool,
    closed: bool,
    failed: bool,
    failure: Option<String>,
}

/// Reject handles with a different provider or malformed native session identity.
pub(super) fn validate_handle(handle: &AgentPersistenceHandle) -> Result<(), AgentSessionError> {
    if handle.provider != PROVIDER
        || handle.session_id.is_empty()
        || handle.session_id.len() > 1024
        || handle.session_id.chars().any(char::is_control)
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Decode bounded legacy or ACP input correlations; malformed handles are rejected.
pub(super) fn clients(
    handle: &AgentPersistenceHandle,
) -> Result<BTreeMap<String, String>, AgentSessionError> {
    let native = match &handle.native_handle {
        Some(Value::String(text)) if text.len() <= 16 * 1024 * 1024 + 256 * 1024 => {
            serde_json::from_str(text).map_err(|_| AgentSessionError::Rejected)?
        }
        Some(Value::Object(_)) => handle
            .native_handle
            .clone()
            .ok_or(AgentSessionError::Rejected)?,
        None => handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("opencode"))
            .cloned()
            .unwrap_or(Value::Null),
        _ => return Err(AgentSessionError::Rejected),
    };
    let clients: BTreeMap<String, String> = native
        .get("clients")
        .map_or(Ok(BTreeMap::new()), |clients| {
            serde_json::from_value(clients.clone())
        })
        .map_err(|_| AgentSessionError::Rejected)?;
    if clients.len() > 4096
        || clients.iter().any(|(id, client)| {
            [id, client].iter().any(|text| {
                text.is_empty() || text.len() > 1024 || text.chars().any(char::is_control)
            })
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(clients)
}

/// Create or load an owned ACP session for the supplied spec and optional resume purpose.
/// Invalid configuration rejects; native initialization and replay errors are propagated.
pub(super) async fn open(
    client: &OpenCodeClient,
    spec: &AgentSessionSpec,
    binding: Option<(&AgentPersistenceHandle, AgentResumePurpose)>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    if let Some((handle, _)) = binding {
        validate_handle(handle)?;
        clients(handle)?;
    }
    let history_only = binding.is_some_and(|(_, purpose)| purpose == AgentResumePurpose::History);
    let inherited = StoredAgentConfig::default();
    let (mut transport, capabilities) = launcher::spawn(
        client,
        &spec.cwd,
        if history_only {
            &inherited
        } else {
            &spec.config
        },
    )
    .await?;
    let (id, options, users, clients) = if let Some((handle, _)) = binding {
        validate_handle(handle)?;
        if capabilities["loadSession"] != true {
            return Err(AgentSessionError::Unavailable);
        }
        let (options, stream) = history::replay(
            &mut transport,
            &handle.session_id,
            &spec.cwd,
            client.images.clone(),
        )
        .await?;
        let clients = clients(handle)?;
        (handle.session_id.clone(), options, stream.users, clients)
    } else {
        let result = transport
            .request("session/new", json!({"cwd":spec.cwd,"mcpServers":[]}))
            .await?;
        (
            config::text(&result, "sessionId")?.to_owned(),
            config::state(&result)?,
            Vec::new(),
            BTreeMap::new(),
        )
    };
    let mut options = options;
    if !history_only {
        config::apply(&mut transport, &id, &mut options, &spec.config).await?;
    }
    let info = config::runtime(&id, &options);
    let (_, events) = mpsc::channel(1);
    Ok(Session {
        client: client.clone(),
        spec: spec.clone(),
        id,
        info,
        clients: clients.clone(),
        connection: Some(Connection {
            transport,
            options,
            capabilities,
            clients,
            users: users.into_iter().collect(),
        }),
        pending: Arc::default(),
        commands: None,
        events,
        finished: None,
        task: None,
        queued: VecDeque::new(),
        turn: None,
        history_only,
        closed: false,
        failed: false,
        failure: None,
    })
}

impl Session {
    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        selected: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        config::validate(selected)?;
        let blocks = prompt.blocks()?;
        if self.closed
            || self.failed
            || self.history_only
            || self.turn.is_some()
            || prompt.output_schema.is_some()
            || self.clients.len() >= 4096
        {
            return Err(AgentSessionError::Rejected);
        }
        let turn = prompt
            .client_message_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let submitted = Stream::submitted(self.client.images.clone(), &turn, &blocks)?;
        let mut connection = self.connection.take().ok_or(AgentSessionError::Failed)?;
        if !prompt.images.is_empty()
            && connection.capabilities["promptCapabilities"]["image"] != true
        {
            self.connection = Some(connection);
            return Err(AgentSessionError::Rejected);
        }
        if let Err(error) = self.prepare(&mut connection, selected).await {
            self.info = config::runtime(&self.id, &connection.options);
            self.connection = Some(connection);
            return Err(error);
        }
        let rpc_id = match connection
            .transport
            .begin(
                "session/prompt",
                json!({"sessionId":self.id,"messageId":turn,"prompt":blocks}),
            )
            .await
        {
            Ok(id) => id,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        self.info = config::runtime(&self.id, &connection.options);
        self.spec.config = selected.clone();
        self.failure = None;
        let (sender, events) = mpsc::channel(128);
        self.events = events;
        let (commands, receiver) = mpsc::channel(128);
        self.commands = Some(commands);
        let (finished, completion) = oneshot::channel();
        self.finished = Some(completion);
        let execution = Execution {
            client: self.client.clone(),
            id: self.id.clone(),
            cwd: self.spec.cwd.clone(),
            rpc_id,
            turn: turn.clone(),
            submitted,
            pending: self.pending.clone(),
            events: sender,
            commands: receiver,
        };
        self.task = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let pending = execution.pending.clone();
            let events = execution.events.clone();
            let result = execution.run(connection).await;
            if result.is_err() {
                let ids = pending
                    .lock()
                    .map(|mut requests| {
                        std::mem::take(&mut *requests)
                            .into_keys()
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                for id in ids {
                    let _ = events.send(AgentTurnEvent::PermissionResolved(id)).await;
                }
            }
            let _ = finished.send(result);
        })));
        self.turn = Some(turn.clone());
        Ok(turn)
    }

    async fn prepare(
        &self,
        connection: &mut Connection,
        selected: &StoredAgentConfig,
    ) -> Result<(), AgentSessionError> {
        if config::permission(selected) != config::permission(&self.spec.config)
            || selected.system_prompt != self.spec.config.system_prompt
            || (selected.system_prompt.is_some() && selected.mode_id != self.spec.config.mode_id)
        {
            let (mut transport, capabilities) =
                launcher::spawn(&self.client, &self.spec.cwd, selected).await?;
            let mut options = if capabilities["sessionCapabilities"]["resume"].is_object() {
                let result = transport
                    .request(
                        "session/resume",
                        json!({"sessionId":self.id,"cwd":self.spec.cwd,"mcpServers":[]}),
                    )
                    .await?;
                config::state(&result)?
            } else if capabilities["loadSession"] == true {
                history::replay(
                    &mut transport,
                    &self.id,
                    &self.spec.cwd,
                    self.client.images.clone(),
                )
                .await?
                .0
            } else {
                return Err(AgentSessionError::Unavailable);
            };
            config::apply(&mut transport, &self.id, &mut options, selected).await?;
            connection.transport.close().await?;
            connection.options = options;
            connection.capabilities = capabilities;
            connection.transport = transport;
            return Ok(());
        }
        config::apply(
            &mut connection.transport,
            &self.id,
            &mut connection.options,
            selected,
        )
        .await?;
        Ok(())
    }

    async fn command(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<(), AgentSessionError>>) -> Command,
    ) -> Result<(), AgentSessionError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .as_ref()
            .ok_or(AgentSessionError::Rejected)?
            .send(command(sender))
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        tokio::time::timeout(self.client.deadline, receiver)
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?
    }
}

struct Execution {
    client: OpenCodeClient,
    id: String,
    cwd: String,
    rpc_id: String,
    turn: String,
    submitted: Vec<crate::protocol::timeline::NativeItem>,
    pending: Pending,
    events: mpsc::Sender<AgentTurnEvent>,
    commands: mpsc::Receiver<Command>,
}

impl Execution {
    async fn emit(&self, event: AgentTurnEvent) -> Result<(), AgentSessionError> {
        self.events
            .send(event)
            .await
            .map_err(|_| AgentSessionError::Failed)
    }

    async fn run(mut self, mut connection: Connection) -> Result<Finished, AgentSessionError> {
        let mut stream = Stream::for_turn(self.client.images.clone(), self.turn.clone());
        self.emit(AgentTurnEvent::RuntimeInfo(config::runtime(
            &self.id,
            &connection.options,
        )))
        .await?;
        for entry in std::mem::take(&mut self.submitted) {
            self.emit(AgentTurnEvent::Timeline(entry)).await?;
        }
        let (terminal, failure) = loop {
            let message = tokio::select! {
                command = self.commands.recv() => {
                    let command = command.ok_or(AgentSessionError::Failed)?;
                    self.respond(&mut connection.transport, command).await;
                    continue;
                }
                message = connection.transport.receive() => message?,
            };
            if message.get("method").is_none() {
                if message["id"] != self.rpc_id {
                    return Err(AgentSessionError::Failed);
                }
                break match acp_transport::response(&message) {
                    Ok(result) => (
                        match result["stopReason"].as_str() {
                            Some("cancelled") => AgentTurnEvent::Cancelled,
                            Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                                AgentTurnEvent::Completed(None)
                            }
                            _ => return Err(AgentSessionError::Failed),
                        },
                        None,
                    ),
                    Err(AgentSessionError::Rejected) => (
                        AgentTurnEvent::Failed,
                        // OpenCode's ACP error message is its native user-facing explanation.
                        // Diagnostic data can include HTTP bodies and headers; retain none of it.
                        message["error"]["message"]
                            .as_str()
                            .filter(|text| !text.trim().is_empty() && text.len() <= 4096)
                            .map(str::to_owned),
                    ),
                    Err(error) => return Err(error),
                };
            }
            match message["method"].as_str() {
                Some("session/update") => {
                    if message["params"]["sessionId"] != self.id {
                        return Err(AgentSessionError::Failed);
                    }
                    let update = &message["params"]["update"];
                    if update["sessionUpdate"] == "config_option_update" {
                        connection.options = config::state(update)?;
                        self.emit(AgentTurnEvent::RuntimeInfo(config::runtime(
                            &self.id,
                            &connection.options,
                        )))
                        .await?;
                    } else {
                        stream.update(update)?;
                        for event in stream.events.drain(..) {
                            // Completion is published only after native replay confirms the prompt.
                            if !matches!(event, AgentTurnEvent::Timeline(_))
                                && !matches!(&event, AgentTurnEvent::Progress { entry, .. } if entry.item["type"] == "user_message")
                            {
                                self.emit(event).await?;
                            }
                        }
                    }
                }
                Some("session/request_permission" | "elicitation/create") => {
                    self.capture(&mut connection.transport, &message).await?;
                }
                Some("$/cancel_request") => {
                    let id = self
                        .pending
                        .lock()
                        .map_err(|_| AgentSessionError::Failed)?
                        .iter()
                        .find(|(_, pending)| pending.rpc_id == message["params"]["requestId"])
                        .map(|(id, _)| id.clone());
                    if let Some(id) = id {
                        self.pending
                            .lock()
                            .map_err(|_| AgentSessionError::Failed)?
                            .remove(&id);
                        self.emit(AgentTurnEvent::PermissionResolved(id)).await?;
                    }
                }
                Some(_) if message.get("id").is_some() => {
                    connection.transport.send(&json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32601,"message":"Unsupported client method"}})).await?;
                }
                _ => {}
            }
        };
        self.settle(connection, terminal, failure).await
    }

    async fn settle(
        &self,
        mut connection: Connection,
        mut event: AgentTurnEvent,
        failure: Option<String>,
    ) -> Result<Finished, AgentSessionError> {
        let pending =
            std::mem::take(&mut *self.pending.lock().map_err(|_| AgentSessionError::Failed)?);
        for id in pending.into_keys() {
            self.emit(AgentTurnEvent::PermissionResolved(id)).await?;
        }
        let (options, mut replay) = history::replay(
            &mut connection.transport,
            &self.id,
            &self.cwd,
            self.client.images.clone(),
        )
        .await?;
        let new = replay
            .users
            .iter()
            .filter(|id| !connection.users.contains(id.as_str()))
            .collect::<Vec<_>>();
        if new.len() > 1 {
            return Err(AgentSessionError::Failed);
        }
        // Native commands such as /compact may complete without inserting a user message.
        if let Some(user) = new.first() {
            connection
                .clients
                .insert((*user).clone(), self.turn.clone());
        }
        connection.users = replay.users.iter().cloned().collect();
        let last_message = if new.is_empty() {
            None
        } else {
            replay.last_message.clone()
        };
        self.emit(AgentTurnEvent::History(history::entries(
            &mut replay,
            &connection.clients,
        )))
        .await?;
        connection.options = options;
        if matches!(event, AgentTurnEvent::Completed(_)) {
            event = AgentTurnEvent::Completed(last_message);
        }
        Ok(Finished {
            connection,
            event,
            failure,
        })
    }

    async fn capture(
        &self,
        transport: &mut Transport,
        message: &Value,
    ) -> Result<(), AgentSessionError> {
        if message["params"]["sessionId"] != self.id {
            return Err(AgentSessionError::Failed);
        }
        let pending = if message["method"] == "elicitation/create" {
            match interactions::form(message) {
                Ok(pending) => pending,
                Err(AgentSessionError::Rejected) => {
                    transport.send(&json!({"jsonrpc":"2.0","id":message["id"],"result":{"action":"cancel"}})).await?;
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
        } else {
            interactions::capture(message)?
        };
        let id = config::text(&pending.request, "id")?.to_owned();
        let request = pending.request.clone();
        {
            let mut requests = self.pending.lock().map_err(|_| AgentSessionError::Failed)?;
            if requests.len() >= 128
                || requests
                    .values()
                    .any(|previous| previous.rpc_id == pending.rpc_id)
            {
                return Err(AgentSessionError::Failed);
            }
            requests.insert(id, pending);
        }
        self.emit(AgentTurnEvent::PermissionRequested(request))
            .await
    }

    async fn respond(&self, transport: &mut Transport, command: Command) {
        match command {
            Command::Cancel(sender) => {
                let result = transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await;
                let _ = sender.send(result);
            }
            Command::Reply(id, response, sender) => {
                let reply = self
                    .pending
                    .lock()
                    .map_err(|_| AgentSessionError::Failed)
                    .and_then(|requests| {
                        interactions::resolve(
                            requests.get(&id).ok_or(AgentSessionError::Rejected)?,
                            &response,
                        )
                    });
                let result = async {
                    transport.send(&reply?).await?;
                    self.pending.lock().map_err(|_| AgentSessionError::Failed)?.remove(&id);
                    self.emit(AgentTurnEvent::PermissionResolved(id)).await?;
                    if response["behavior"] == "deny" && response["interrupt"] == true {
                        transport.send(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.id}})).await?;
                    }
                    Ok(())
                }.await;
                let _ = sender.send(result);
            }
        }
    }
}

impl AgentSession for Session {
    fn failure_message(&self) -> Option<&str> {
        self.failure.as_deref()
    }
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Ok(self.info.clone()) })
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let native = json!({"transport":"acp","config":self.spec.config,"model":self.info.model,"clients":self.clients});
        Some(AgentPersistenceHandle {
            provider: PROVIDER.into(),
            session_id: self.id.clone(),
            native_handle: Some(json!(native.to_string())),
            metadata: Some(BTreeMap::from([("cwd".into(), json!(self.spec.cwd))])),
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
    fn cancel_turn<'a>(&'a mut self, turn: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.turn.as_deref() != Some(turn) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            self.command(Command::Cancel).await
        })
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.pending.lock().map_or_else(
            |_| Vec::new(),
            |requests| {
                requests
                    .values()
                    .map(|pending| pending.request.clone())
                    .collect()
            },
        )
    }
    fn respond_permission<'a>(
        &'a mut self,
        id: &'a str,
        response: &'a Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            self.command(|sender| Command::Reply(id.to_owned(), response.clone(), sender))
                .await
        })
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Ok(event) = self.events.try_recv() {
            return Ok(Some(event));
        }
        if let Some(receiver) = &mut self.finished {
            match receiver.try_recv() {
                Ok(Ok(Finished {
                    connection,
                    event,
                    failure,
                })) => {
                    self.info = config::runtime(&self.id, &connection.options);
                    self.clients = connection.clients.clone();
                    self.connection = Some(connection);
                    self.finished = None;
                    self.commands = None;
                    self.task = None;
                    self.failure = failure;
                    self.queued.push_back(event);
                    if let Ok(event) = self.events.try_recv() {
                        return Ok(Some(event));
                    }
                }
                Ok(Err(error)) => {
                    self.failed = true;
                    self.finished = None;
                    self.commands = None;
                    self.task = None;
                    return Err(error);
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.failed = true;
                    return Err(AgentSessionError::Failed);
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
            }
        }
        let event = self.queued.pop_front();
        if matches!(
            event,
            Some(AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed)
        ) {
            self.turn = None;
        }
        Ok(event)
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            if self.closed {
                return Ok(());
            }
            self.closed = true;
            let mut uncertain = false;
            if let Some(commands) = &self.commands {
                let (sender, _acknowledgement) = oneshot::channel();
                if commands.send(Command::Cancel(sender)).await.is_err() {
                    uncertain = true;
                }
            }
            if let Some(mut finished) = self.finished.take() {
                let settled = tokio::time::timeout(std::time::Duration::from_secs(6), async {
                    loop {
                        tokio::select! {
                            result = &mut finished => return result,
                            _ = self.events.recv() => {}
                        }
                    }
                })
                .await;
                match settled {
                    Ok(Ok(Ok(finished))) => {
                        self.connection = Some(finished.connection);
                        self.failure = finished.failure;
                    }
                    _ => uncertain = true,
                }
            }
            self.events.close();
            if let Some(mut task) = self.task.take() {
                if !task.is_finished() {
                    task.abort();
                }
                let _ = tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await;
            }
            self.pending
                .lock()
                .map_err(|_| AgentSessionError::Failed)?
                .clear();
            if let Some(mut connection) = self.connection.take() {
                if connection.capabilities["sessionCapabilities"]["close"].is_object() {
                    let result = connection
                        .transport
                        .request("session/close", json!({"sessionId":self.id}))
                        .await;
                    if result.is_err() {
                        uncertain = true;
                    }
                }
                connection.transport.close().await?;
            }
            if uncertain {
                Err(AgentSessionError::Failed)
            } else {
                Ok(())
            }
        })
    }
}
