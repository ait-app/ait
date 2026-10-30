//! One prompt request, bidirectional ACP callbacks and native replay before writer release.
use std::borrow::Cow;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::{
    sync::{mpsc, oneshot, watch},
    time::Instant,
};
use tokio_util::task::AbortOnDropHandle;

use super::{
    OpenCodeClient, PROVIDER, config, history, interactions, launcher,
    observation::{Observation, Source},
    streaming::Stream,
};
use crate::ports::diagnostics::{Category, Diagnostic};
use crate::{
    local::{
        acp_permissions,
        acp_transport::{self, Transport},
    },
    ports::agent_session::{
        AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
    },
    protocol::prompt::AgentPrompt,
};

type Pending = Arc<Mutex<BTreeMap<String, acp_permissions::Pending>>>;

// Native 2.1 drains interrupted tools for up to five seconds before returning its prompt result.
const CANCEL_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Connection {
    transport: Transport,
    /// Directory the native session is filed under; `session_move` can change it.
    location: String,
    options: Value,
    capabilities: Value,
    clients: BTreeMap<String, String>,
    identities: BTreeSet<String>,
    observation: Option<Observation>,
    reported_usage: bool,
}

#[derive(Debug)]
struct Finished {
    connection: Connection,
    event: AgentTurnEvent,
    failure: Option<String>,
}

#[derive(Debug)]
struct Command {
    id: String,
    response: Value,
    confirmation: oneshot::Sender<Result<(), AgentSessionError>>,
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
    cancellation: Option<watch::Sender<Option<Instant>>>,
    events: mpsc::Receiver<AgentTurnEvent>,
    finished: Option<oneshot::Receiver<Result<Finished, AgentSessionError>>>,
    task: Option<AbortOnDropHandle<()>>,
    queued: VecDeque<AgentTurnEvent>,
    initial: VecDeque<AgentTurnEvent>,
    turn: Option<String>,
    history_only: bool,
    closed: bool,
    failed: bool,
    failure: Option<String>,
    diagnostics: crate::local::diagnostics::Diagnostics,
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
    let payload = handle
        .native_handle
        .as_ref()
        .filter(|native| native.as_str() != Some(handle.session_id.as_str()));
    let native = match payload {
        Some(Value::String(text)) if text.len() <= 16 * 1024 * 1024 + 256 * 1024 => {
            serde_json::from_str(text).map_err(|_| AgentSessionError::Rejected)?
        }
        Some(native @ Value::Object(_)) => native.clone(),
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
    let selected = if history_only {
        &inherited
    } else {
        &spec.config
    };
    let (mut transport, capabilities, source) =
        launcher::observed(client, &spec.cwd, selected).await?;
    let (id, mut options, mut identities, clients, location) = if let Some((handle, _)) = binding {
        if capabilities["loadSession"] != true {
            return Err(AgentSessionError::Unavailable);
        }
        let (options, identities, location) =
            reload(client, &mut transport, handle, &spec.cwd).await?;
        let clients = clients(handle)?;
        (
            handle.session_id.clone(),
            options,
            identities,
            clients,
            location,
        )
    } else {
        let result = transport
            .startup_request(
                "session/new",
                json!({"cwd":spec.cwd,"mcpServers":[]}),
                client.startup_timeout,
            )
            .await?;
        (
            config::text(&result, "sessionId")?.to_owned(),
            config::state(&result)?,
            BTreeSet::new(),
            BTreeMap::new(),
            spec.cwd.clone(),
        )
    };
    if !history_only
        && let Err(error) = config::apply(&mut transport, &id, &mut options, &spec.config).await
    {
        if binding.is_none() && capabilities["sessionCapabilities"]["delete"].is_object() {
            // A rejected initial selection must not leave an empty native session behind.
            // Loaded sessions belong to the user and are never deleted on a configuration error.
            let mut temporary = super::summary::Temporary::new(client, &spec.cwd, transport);
            temporary.id = Some(id);
            temporary.close().await?;
        }
        return Err(error);
    }
    let info = config::runtime(&id, &options);
    let mut observation = observe(source, &id, &location).await;
    let initial = initial_usage(client, &mut observation)?;
    if let Some(observation) = &observation {
        identities.extend(observation.users().iter().cloned());
    }
    let (_, events) = mpsc::channel(1);
    let diagnostics = transport.diagnostics.clone();
    Ok(Session {
        diagnostics,
        client: client.clone(),
        spec: spec.clone(),
        id,
        info,
        clients: clients.clone(),
        connection: Some(Connection {
            transport,
            location,
            options,
            capabilities,
            clients,
            identities,
            observation,
            reported_usage: false,
        }),
        pending: Arc::default(),
        commands: None,
        cancellation: None,
        events,
        finished: None,
        task: None,
        queued: VecDeque::new(),
        initial,
        turn: None,
        history_only,
        closed: false,
        failed: false,
        failure: None,
    })
}

/// Replay a bound native session; returns its options, accepted inputs and current location.
async fn reload(
    client: &OpenCodeClient,
    transport: &mut Transport,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<(Value, BTreeSet<String>, String), AgentSessionError> {
    let (options, stream, location) = history::replay(
        transport,
        &handle.session_id,
        cwd,
        client.images.clone(),
        client.startup_timeout,
    )
    .await?;
    Ok((options, history::identities(&stream), location))
}

fn initial_usage(
    client: &OpenCodeClient,
    observation: &mut Option<Observation>,
) -> Result<VecDeque<AgentTurnEvent>, AgentSessionError> {
    let mut stream = Stream::new(client.images.clone());
    if let Some(observation) = observation {
        for update in observation.initial.drain(..) {
            stream.update(&update)?;
        }
    }
    Ok(stream.events)
}

async fn observe(source: Option<Source>, id: &str, cwd: &str) -> Option<Observation> {
    match source {
        Some(source) => {
            if let Ok(observation) = source.bind(id, cwd).await {
                Some(observation)
            } else {
                tracing::warn!(
                    "OpenCode native observation unavailable; ACP execution is unchanged"
                );
                None
            }
        }
        None => None,
    }
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
        self.diagnostics = connection.transport.diagnostics.clone();
        self.diagnostics.clear();
        if let Some(observation) = &mut connection.observation {
            observation.begin();
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
        let (cancellation, cancelled) = watch::channel(None);
        self.cancellation = Some(cancellation.clone());
        let (finished, completion) = oneshot::channel();
        self.finished = Some(completion);
        let execution = Execution {
            client: self.client.clone(),
            id: self.id.clone(),
            rpc_id,
            turn: turn.clone(),
            submitted,
            pending: self.pending.clone(),
            events: sender,
            commands: receiver,
            cancellation,
            cancelled,
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
            let (mut transport, capabilities, source) =
                launcher::observed(&self.client, &self.spec.cwd, selected).await?;
            let location = &connection.location;
            let mut options = if capabilities["sessionCapabilities"]["resume"].is_object() {
                let result = transport
                    .startup_request(
                        "session/resume",
                        json!({"sessionId":self.id,"cwd":location,"mcpServers":[]}),
                        self.client.startup_timeout,
                    )
                    .await?;
                config::state(&result)?
            } else if capabilities["loadSession"] == true {
                history::replay(
                    &mut transport,
                    &self.id,
                    location,
                    self.client.images.clone(),
                    self.client.startup_timeout,
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
            connection.observation = observe(source, &self.id, &connection.location).await;
            if let Some(observation) = &connection.observation {
                connection
                    .identities
                    .extend(observation.users().iter().cloned());
            }
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

    async fn command(&self, id: &str, response: &Value) -> Result<(), AgentSessionError> {
        let (sender, receiver) = oneshot::channel();
        self.commands
            .as_ref()
            .ok_or(AgentSessionError::Rejected)?
            .send(Command {
                id: id.to_owned(),
                response: response.clone(),
                confirmation: sender,
            })
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        tokio::time::timeout(self.client.deadline, receiver)
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)?
    }

    fn check_cancellation(&mut self) -> Result<(), AgentSessionError> {
        let requested = self
            .cancellation
            .as_ref()
            .and_then(|sender| *sender.borrow());
        if requested
            .is_some_and(|started| started.elapsed() >= CANCEL_GRACE.min(self.client.deadline))
            && self.task.as_ref().is_some_and(|task| !task.is_finished())
        {
            let message =
                "OpenCode did not confirm cancellation; its native process will be closed.";
            self.diagnostics.observe_failure(
                Diagnostic::new(Category::Timeout, "abort_confirmation"),
                message,
                "session/cancel",
            );
            self.failure = Some(message.to_owned());
            self.failed = true;
            if let Some(task) = &self.task {
                task.abort();
            }
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }
}

fn request_cancellation(sender: &watch::Sender<Option<Instant>>) {
    sender.send_if_modified(|requested| {
        if requested.is_some() {
            return false;
        }
        *requested = Some(Instant::now());
        true
    });
}

struct Execution {
    client: OpenCodeClient,
    id: String,
    rpc_id: String,
    turn: String,
    submitted: Vec<crate::protocol::timeline::NativeItem>,
    pending: Pending,
    events: mpsc::Sender<AgentTurnEvent>,
    commands: mpsc::Receiver<Command>,
    cancellation: watch::Sender<Option<Instant>>,
    cancelled: watch::Receiver<Option<Instant>>,
}

impl Execution {
    async fn emit(&self, event: AgentTurnEvent) -> Result<(), AgentSessionError> {
        self.events
            .send(event)
            .await
            .map_err(|_| AgentSessionError::Failed)
    }

    async fn observed(
        &self,
        stream: &mut Stream,
        updates: Vec<Value>,
    ) -> Result<(), AgentSessionError> {
        for update in updates {
            stream.update(&update)?;
        }
        self.publish(stream).await
    }

    async fn publish(&self, stream: &mut Stream) -> Result<(), AgentSessionError> {
        for event in stream.events.drain(..) {
            // Completion is published only after native replay confirms the prompt.
            if (!matches!(event, AgentTurnEvent::Timeline(_))
                || matches!(&event, AgentTurnEvent::Timeline(entry)
                    if entry.item["type"] == "compaction"))
                && !matches!(&event, AgentTurnEvent::Progress { entry, .. }
                    if entry.item["type"] == "user_message")
            {
                self.emit(event).await?;
            }
        }
        Ok(())
    }

    async fn update(
        &self,
        connection: &mut Connection,
        stream: &mut Stream,
        message: &Value,
    ) -> Result<(), AgentSessionError> {
        if message["params"]["sessionId"] != self.id {
            return Err(AgentSessionError::Failed);
        }
        let update = &message["params"]["update"];
        if update["sessionUpdate"] == "usage_update" {
            if update["used"] == 0
                && (connection.reported_usage
                    || connection
                        .observation
                        .as_ref()
                        .is_some_and(Observation::has_usage))
            {
                return Ok(());
            }
            connection.reported_usage |= update["used"].as_u64().is_some_and(|used| used > 0);
        }
        if update["sessionUpdate"] == "config_option_update" {
            connection.options = config::state(update)?;
            self.emit(AgentTurnEvent::RuntimeInfo(config::runtime(
                &self.id,
                &connection.options,
            )))
            .await?;
        } else {
            stream.update(update)?;
            self.publish(stream).await?;
        }
        Ok(())
    }

    async fn receive(
        &mut self,
        connection: &mut Connection,
        stream: &mut Stream,
    ) -> Result<Value, AgentSessionError> {
        loop {
            if self
                .cancelled
                .has_changed()
                .map_err(|_| AgentSessionError::Failed)?
            {
                self.send_cancellation(&mut connection.transport).await?;
            }
            tokio::select! {
                changed = self.cancelled.changed() => {
                    changed.map_err(|_| AgentSessionError::Failed)?;
                    self.send_cancellation(&mut connection.transport).await?;
                }
                command = self.commands.recv() => {
                    let command = command.ok_or(AgentSessionError::Failed)?;
                    self.respond(&mut connection.transport, command).await;
                }
                updates = async {
                    match &mut connection.observation {
                        Some(observation) => observation.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(updates) = updates {
                        self.observed(stream, updates).await?;
                    } else {
                        tracing::warn!("OpenCode observation refresh failed; ACP remains active");
                    }
                },
                message = connection.transport.receive() => return message,
            }
        }
    }

    async fn send_cancellation(
        &mut self,
        transport: &mut Transport,
    ) -> Result<(), AgentSessionError> {
        self.cancelled.borrow_and_update();
        transport
            .send(&json!({"jsonrpc":"2.0","method":"session/cancel",
            "params":{"sessionId":self.id}}))
            .await
    }

    async fn refresh(
        &self,
        connection: &mut Connection,
        stream: &mut Stream,
    ) -> Result<(), AgentSessionError> {
        if let Some(observation) = &mut connection.observation {
            if let Ok(updates) = observation.refresh(false).await {
                self.observed(stream, updates).await?;
            } else {
                tracing::warn!("OpenCode final observation refresh failed");
            }
            self.observed(stream, observation.finish()?).await?;
        }
        Ok(())
    }

    fn failure(connection: &Connection, error: &Value) -> String {
        let diagnostic = crate::local::diagnostics::native(error, "rpc");
        connection
            .transport
            .diagnostics
            .observe_native(error, "rpc", "session/prompt");
        connection
            .transport
            .diagnostics
            .message()
            .unwrap_or_else(|| diagnostic.category.message().to_owned())
    }

    fn terminal(result: &Value) -> Result<(AgentTurnEvent, Option<String>), AgentSessionError> {
        Ok(match result["stopReason"].as_str() {
            Some("cancelled") => (AgentTurnEvent::Cancelled, None),
            Some("end_turn") => (AgentTurnEvent::Completed(None), None),
            Some("max_tokens") => (
                AgentTurnEvent::Failed,
                Some("OpenCode reached the model output limit. Send Continue to resume.".into()),
            ),
            Some("max_turn_requests") => (
                AgentTurnEvent::Failed,
                Some("OpenCode reached its turn request limit. Send Continue to resume.".into()),
            ),
            Some("refusal") => (
                AgentTurnEvent::Failed,
                Some("OpenCode refused this request. Revise it before continuing.".into()),
            ),
            _ => return Err(AgentSessionError::Failed),
        })
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
        if let Some(observation) = &mut connection.observation {
            let updates = std::mem::take(&mut observation.initial);
            self.observed(&mut stream, updates).await?;
        }
        let (mut terminal, mut failure) = loop {
            let message = self.receive(&mut connection, &mut stream).await?;
            if message.get("method").is_none() {
                if message["id"] != self.rpc_id {
                    return Err(AgentSessionError::Failed);
                }
                break match acp_transport::response(&message) {
                    Ok(result) => Self::terminal(&result)?,
                    Err(AgentSessionError::Rejected) => (
                        AgentTurnEvent::Failed,
                        // Native error payloads may include prompts, HTTP bodies and headers.
                        // Only fixed classified explanations cross the application boundary.
                        Some(Self::failure(&connection, &message["error"])),
                    ),
                    Err(error) => return Err(error),
                };
            }
            match message["method"].as_str() {
                Some("session/update") => {
                    self.update(&mut connection, &mut stream, &message).await?;
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
        self.refresh(&mut connection, &mut stream).await?;
        if matches!(terminal, AgentTurnEvent::Completed(_))
            && connection
                .observation
                .as_ref()
                .is_some_and(Observation::unknown_finish)
        {
            terminal = AgentTurnEvent::Failed;
            failure = Some(
                "OpenCode ended without a valid finish reason. Send Continue to resume.".into(),
            );
        }
        stream.finish_compactions();
        for event in stream.events.drain(..) {
            self.emit(event).await?;
        }
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
        let (options, mut replay, location) = history::replay(
            &mut connection.transport,
            &self.id,
            &connection.location,
            self.client.images.clone(),
            self.client.deadline,
        )
        .await?;
        connection.location = location;
        let has_input = replay
            .users
            .iter()
            .any(|id| !connection.identities.contains(id));
        let mut identities = history::identities(&replay);
        if let Some(observation) = &connection.observation {
            identities.extend(observation.users().iter().cloned());
        }
        let new = identities
            .difference(&connection.identities)
            .collect::<Vec<_>>();
        if connection.clients.len() + new.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
        // Native commands such as /compact may complete without inserting a user message.
        // Compactions and synthetic continuation inputs retain this accepted prompt's identity.
        for user in &new {
            connection
                .clients
                .insert((*user).clone(), self.turn.clone());
        }
        let last_message = if has_input {
            replay.last_message.clone()
        } else {
            None
        };
        connection.identities = identities;
        let mut entries = history::entries(&mut replay, &connection.clients);
        if let Some(observation) = &connection.observation {
            observation.history(&mut entries, &connection.clients);
        }
        self.emit(AgentTurnEvent::History(entries)).await?;
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
            acp_permissions::capture(message, PROVIDER)?
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
        let Command {
            id,
            response,
            confirmation,
        } = command;
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
            self.pending
                .lock()
                .map_err(|_| AgentSessionError::Failed)?
                .remove(&id);
            self.emit(AgentTurnEvent::PermissionResolved(id)).await?;
            if response["behavior"] == "deny" && response["interrupt"] == true {
                request_cancellation(&self.cancellation);
            }
            Ok(())
        }
        .await;
        let _ = confirmation.send(result);
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn failure_diagnostic(&self) -> Option<Diagnostic> {
        self.diagnostics.failure()
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.failure.as_deref().map(Cow::Borrowed)
    }
    fn provider(&self) -> &'static str {
        PROVIDER
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        Ok(self.info.clone())
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        let native = json!({"transport":"acp","config":self.spec.config,"model":self.info.model,"clients":self.clients});
        Some(AgentPersistenceHandle {
            provider: PROVIDER.into(),
            session_id: self.id.clone(),
            native_handle: Some(json!(self.id)),
            metadata: Some(BTreeMap::from([
                ("cwd".into(), json!(self.spec.cwd)),
                ("opencode".into(), native),
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
    async fn cancel_turn(&mut self, turn: &str) -> Result<(), AgentSessionError> {
        if self.turn.as_deref() != Some(turn) || self.closed {
            return Err(AgentSessionError::Rejected);
        }
        request_cancellation(
            self.cancellation
                .as_ref()
                .ok_or(AgentSessionError::Rejected)?,
        );
        Ok(())
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
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        self.command(id, response).await
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        self.check_cancellation()?;
        if let Some(event) = self.initial.pop_front() {
            return Ok(Some(event));
        }
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
                    self.cancellation = None;
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
                    self.cancellation = None;
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
    async fn close(&mut self) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let failed_cancellation = self.failed
            && self
                .cancellation
                .as_ref()
                .is_some_and(|sender| sender.borrow().is_some());
        let mut uncertain = false;
        if let Some(cancellation) = &self.cancellation {
            request_cancellation(cancellation);
        }
        if let Some(mut finished) = self.finished.take() {
            let settled = tokio::time::timeout(Duration::from_secs(6), async {
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
                    if self.failure.is_none() {
                        self.failure = finished.failure;
                    }
                }
                _ => uncertain = true,
            }
        }
        self.events.close();
        if let Some(mut task) = self.task.take() {
            if !task.is_finished() {
                task.abort();
            }
            if tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
            {
                uncertain = true;
            } else if failed_cancellation {
                // Joining the failed owner confirms teardown, not successful prompt settlement.
                // Its cancellation failure must now be allowed to leave the host's running state.
                uncertain = false;
            }
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
    }
}

#[cfg(all(test, unix))]
mod tests;
