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
    known: BTreeMap<String, crate::protocol::timeline::NativeItem>,
    clients: BTreeMap<String, String>,
    users: BTreeSet<String>,
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
    finished: Option<oneshot::Receiver<Result<(Connection, AgentTurnEvent), AgentSessionError>>>,
    task: Option<AbortOnDropHandle<()>>,
    queued: VecDeque<AgentTurnEvent>,
    turn: Option<String>,
    history_only: bool,
    closed: bool,
    failed: bool,
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
    let (id, options, known, users, clients) = if let Some((handle, _)) = binding {
        validate_handle(handle)?;
        if capabilities["loadSession"] != true {
            return Err(AgentSessionError::Unavailable);
        }
        let (options, mut stream) = history::replay(
            &mut transport,
            &handle.session_id,
            &spec.cwd,
            client.images.clone(),
        )
        .await?;
        let clients = clients(handle)?;
        let known = history::entries(&mut stream, &clients)
            .into_iter()
            .map(|entry| (entry.key.clone(), entry))
            .collect();
        (
            handle.session_id.clone(),
            options,
            known,
            stream.users,
            clients,
        )
    } else {
        let result = transport
            .request("session/new", json!({"cwd":spec.cwd,"mcpServers":[]}))
            .await?;
        (
            config::text(&result, "sessionId")?.to_owned(),
            config::state(&result)?,
            BTreeMap::new(),
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
            known,
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
            || (prompt.client_message_id.is_some() && self.clients.len() >= 4096)
        {
            return Err(AgentSessionError::Rejected);
        }
        let mut connection = self.connection.take().ok_or(AgentSessionError::Failed)?;
        if !prompt.images.is_empty()
            && connection.capabilities["promptCapabilities"]["image"] != true
        {
            self.connection = Some(connection);
            return Err(AgentSessionError::Rejected);
        }
        let prepared = async {
            self.prepare(&mut connection, selected).await?;
            connection
                .transport
                .begin(
                    "session/prompt",
                    json!({"sessionId":self.id,"prompt":blocks}),
                )
                .await
        }
        .await;
        let rpc_id = match prepared {
            Ok(id) => id,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        self.info = config::runtime(&self.id, &connection.options);
        self.spec.config = selected.clone();
        let turn = uuid::Uuid::new_v4().to_string();
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
            client_message: prompt.client_message_id.clone(),
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
        self.queued
            .push_back(AgentTurnEvent::RuntimeInfo(self.info.clone()));
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
            connection.transport.close().await?;
            let (mut transport, capabilities) =
                launcher::spawn(&self.client, &self.spec.cwd, selected).await?;
            if !capabilities["sessionCapabilities"]["resume"].is_object() {
                return Err(AgentSessionError::Unavailable);
            }
            let result = transport
                .request(
                    "session/resume",
                    json!({"sessionId":self.id,"cwd":self.spec.cwd,"mcpServers":[]}),
                )
                .await?;
            connection.options = config::state(&result)?;
            connection.capabilities = capabilities;
            connection.transport = transport;
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
    client_message: Option<String>,
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

    async fn run(
        mut self,
        mut connection: Connection,
    ) -> Result<(Connection, AgentTurnEvent), AgentSessionError> {
        let mut stream = Stream::new(self.client.images.clone());
        let result = loop {
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
                break acp_transport::response(&message)?;
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
                            if !matches!(event, AgentTurnEvent::Timeline(_)) {
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
        self.settle(connection, &result).await
    }

    async fn settle(
        &self,
        mut connection: Connection,
        result: &Value,
    ) -> Result<(Connection, AgentTurnEvent), AgentSessionError> {
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
        if let Some(client) = &self.client_message {
            let new = replay
                .users
                .iter()
                .filter(|id| !connection.users.contains(id.as_str()))
                .collect::<Vec<_>>();
            if new.len() != 1 {
                return Err(AgentSessionError::Failed);
            }
            connection.clients.insert(new[0].clone(), client.clone());
        }
        connection.users = replay.users.iter().cloned().collect();
        let last_message = replay.last_message.clone();
        for entry in history::entries(&mut replay, &connection.clients) {
            if let Some(previous) = connection.known.get(&entry.key) {
                if previous.item != entry.item || previous.turn_id != entry.turn_id {
                    return Err(AgentSessionError::Failed);
                }
            } else {
                connection.known.insert(entry.key.clone(), entry.clone());
                self.emit(AgentTurnEvent::Timeline(entry)).await?;
            }
        }
        connection.options = options;
        let event = match result["stopReason"].as_str() {
            Some("cancelled") => AgentTurnEvent::Cancelled,
            Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                AgentTurnEvent::Completed(last_message)
            }
            _ => return Err(AgentSessionError::Failed),
        };
        Ok((connection, event))
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
                Ok(Ok((connection, event))) => {
                    self.info = config::runtime(&self.id, &connection.options);
                    self.clients = connection.clients.clone();
                    self.connection = Some(connection);
                    self.finished = None;
                    self.commands = None;
                    self.task = None;
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
                    Ok(Ok(Ok((connection, _)))) => self.connection = Some(connection),
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
