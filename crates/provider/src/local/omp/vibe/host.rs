//! Ait-owned Vibe orchestration over the public RPC of the user's external OMP installation.

mod tools;
mod worker;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::task::AbortOnDropHandle;

use super::super::{
    OmpClient,
    transport::{Transport, Writer},
};
use crate::ports::agent_session::{
    AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::ports::controls::{NativeSubagent, SubagentEvent};

const MAX_WORKERS: usize = 32;
const MAX_CALLS: usize = 8;
pub(in crate::local::omp) const COMMAND: &str = "ait-vibe-mode-v1";

/// Bounded worker state with the external OMP journal needed for a verified resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Record {
    child: NativeSubagent,
    cli: String,
    config: StoredAgentConfig,
    answer: String,
}

impl Record {
    fn status(&mut self, status: &str) {
        self.child.descriptor["status"] = json!(status);
        self.child.descriptor["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    }
    fn view(&self) -> Value {
        json!({"id":self.child.id,"parentId":self.child.parent_id,"status":self.child.descriptor["status"],
            "description":self.child.descriptor["description"],"model":self.child.descriptor["model"],"answer":self.answer})
    }
}

#[derive(Debug)]
struct Slot {
    commands: mpsc::Sender<worker::Command>,
    _task: AbortOnDropHandle<()>,
}

#[derive(Debug, Default)]
struct Registry {
    records: BTreeMap<String, Record>,
    slots: BTreeMap<String, Slot>,
    starting: BTreeMap<String, Option<AbortOnDropHandle<()>>>,
}

#[derive(Debug)]
struct Shared {
    registry: Mutex<Registry>,
    enabled: AtomicBool,
    generation: AtomicU64,
    changes: watch::Sender<u64>,
    client: OmpClient,
    spec: AgentSessionSpec,
    parent: String,
    parent_file: String,
    events: mpsc::Sender<(u64, HostEvent)>,
}

/// Configuration for one external worker process and its initial input.
#[derive(Debug)]
pub(super) struct Launch {
    generation: u64,
    client: OmpClient,
    spec: AgentSessionSpec,
    parent: String,
    parent_file: String,
    cli: String,
    name: String,
    prompt: String,
    restore: Option<AgentPersistenceHandle>,
}

impl Launch {
    fn record(&self, handle: &AgentPersistenceHandle) -> Record {
        let now = chrono::Utc::now().to_rfc3339();
        Record {
            child: NativeSubagent {
                id: handle.session_id.clone(),
                parent_id: self.parent.clone(),
                cwd: self.spec.cwd.clone(),
                persistence: Some(handle.clone()),
                descriptor: json!({"id":handle.session_id,"provider":"omp","cwd":self.spec.cwd,"title":self.name,"description":self.name,"toolCallId":null,
                "createdAt":now,"updatedAt":now,"status":"running","model":self.spec.config.model}),
            },
            cli: self.cli.clone(),
            config: self.spec.config.clone(),
            answer: String::new(),
        }
    }
}

/// Bounded messages from worker actors to their owning parent session.
#[derive(Debug)]
pub(super) enum HostEvent {
    Record(Box<Record>),
    Event(AgentTurnEvent),
    Permission {
        host: String,
        worker: String,
        native: String,
        request: Value,
    },
}

/// Connection-scoped coordinator. Dropping its registry aborts every worker writer and RPC job.
#[derive(Debug)]
pub(in crate::local::omp) struct Host {
    shared: Arc<Shared>,
    incoming: mpsc::Receiver<(u64, HostEvent)>,
    calls: BTreeMap<String, AbortOnDropHandle<()>>,
    permissions: BTreeMap<String, (String, String, Value)>,
    models: BTreeMap<String, String>,
    notifications: Vec<Value>,
    delivery: Option<AbortOnDropHandle<()>>,
    lifecycle_events: Vec<AgentTurnEvent>,
}

impl Host {
    /// Retain only this parent's bounded, previously acknowledged worker identities.
    pub(in crate::local::omp) fn new(
        client: &OmpClient,
        spec: &AgentSessionSpec,
        parent: &str,
        parent_file: &str,
        stored: Option<&Value>,
    ) -> Self {
        let (events, incoming) = mpsc::channel(512);
        let (changes, _) = watch::channel(0);
        let records: Vec<Record> = stored
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        let mut registry = Registry::default();
        for mut record in records.into_iter().take(MAX_WORKERS) {
            if record.child.parent_id != parent
                || record.child.cwd != spec.cwd
                || record.child.persistence.as_ref().is_none_or(|handle| {
                    handle.provider != "omp" || handle.session_id != record.child.id
                })
            {
                continue;
            }
            if record.child.descriptor["status"] == "running" {
                record.status("canceled");
            }
            registry.records.insert(record.child.id.clone(), record);
        }
        Self {
            shared: Arc::new(Shared {
                registry: Mutex::new(registry),
                enabled: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                changes,
                client: client.clone(),
                spec: spec.clone(),
                parent: parent.to_owned(),
                parent_file: parent_file.to_owned(),
                events,
            }),
            incoming,
            calls: BTreeMap::new(),
            permissions: BTreeMap::new(),
            models: BTreeMap::new(),
            notifications: Vec::new(),
            delivery: None,
            lifecycle_events: Vec::new(),
        }
    }

    /// Create a coordinator only after its versioned public extension command was discovered.
    /// Invalid native session identities fail initialization instead of guessing ownership.
    pub(in crate::local::omp) fn discover(
        client: &OmpClient,
        spec: &AgentSessionSpec,
        state: &Value,
        commands: &[Value],
        handle: Option<&AgentPersistenceHandle>,
    ) -> Result<Option<Self>, AgentSessionError> {
        if !client.vibe_host
            || !commands.iter().any(|entry| entry["name"] == COMMAND)
            || commands.iter().any(|entry| entry["name"] == "vibe")
        {
            return Ok(None);
        }
        Ok(Some(Self::new(
            client,
            spec,
            super::super::config::text(state, "sessionId")?,
            super::super::config::text(state, "sessionFile")?,
            handle.and_then(|handle| handle.metadata.as_ref()?.get("aitVibeWorkers")),
        )))
    }

    /// Public extension source; only the external OMP executes it, with no runtime dependency added.
    pub(in crate::local::omp) fn extension() -> Result<tempfile::NamedTempFile, AgentSessionError> {
        let mut file = tempfile::Builder::new()
            .prefix("ait-vibe-bridge-")
            .suffix(".js")
            .tempfile()
            .map_err(|_| AgentSessionError::Failed)?;
        file.write_all(include_bytes!("host/extension.ts"))
            .map_err(|_| AgentSessionError::Failed)?;
        file.flush().map_err(|_| AgentSessionError::Failed)?;
        Ok(file)
    }

    /// Change the director's tools through the public extension command and acknowledged host tools.
    pub(in crate::local::omp) async fn toggle(
        &mut self,
        transport: &mut Transport,
        enabled: bool,
    ) -> Result<(), AgentSessionError> {
        if enabled == self.enabled() {
            return Ok(());
        }
        self.delivery.take();
        let state = transport.request("get_state", json!({})).await?;
        if state["isStreaming"] != false
            || state["isCompacting"] != false
            || state["hasPendingAsyncWork"] != false
            || state["queuedMessageCount"].as_u64() != Some(0)
        {
            return Err(AgentSessionError::Busy);
        }
        if enabled {
            let models = match activate(transport).await {
                Ok(models) => models,
                Err(error) => {
                    let restored = transport.local_command(&format!("/{COMMAND} off")).await;
                    let cleared = transport
                        .request("set_host_tools", json!({"tools":[]}))
                        .await;
                    if restored.is_err() || cleared.is_err() {
                        let _ = transport.close().await;
                    }
                    return Err(error);
                }
            };
            self.models = models;
            self.shared.enabled.store(true, Ordering::Release);
        } else {
            self.stop();
            self.shared.enabled.store(false, Ordering::Release);
            transport.local_command(&format!("/{COMMAND} off")).await?;
            transport
                .request("set_host_tools", json!({"tools":[]}))
                .await?;
        }
        Ok(())
    }

    /// Whether this connection has acknowledged director mode and host tools.
    pub(in crate::local::omp) fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }
    /// Project the bounded worker registry for the existing Vibe UI.
    pub(in crate::local::omp) fn snapshot(&self) -> Value {
        let workers = self
            .shared
            .registry
            .lock()
            .map(|registry| {
                registry
                    .records
                    .values()
                    .map(|record| {
                        json!({"id":record.child.id,"parentId":record.child.parent_id,
                        "status":record.child.descriptor["status"],
                        "description":super::preview(&record.child.descriptor["description"],4096),
                        "model":super::preview(&record.child.descriptor["model"],512)})
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        json!({"enabled":self.enabled(),"workers":workers})
    }
    /// Serialize worker handles without enabling the mode after a restart.
    pub(in crate::local::omp) fn persisted(&self) -> Value {
        self.shared
            .registry
            .lock()
            .ok()
            .and_then(|registry| {
                serde_json::to_value(registry.records.values().collect::<Vec<_>>()).ok()
            })
            .unwrap_or_else(|| json!([]))
    }
    /// Return only child identities acknowledged by this parent coordinator.
    pub(in crate::local::omp) fn children(&self) -> Vec<NativeSubagent> {
        self.shared
            .registry
            .lock()
            .map(|registry| {
                registry
                    .records
                    .values()
                    .map(|record| record.child.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Run a validated native-action operation independently of the parent Agent's lane.
    pub(in crate::local::omp) fn control(
        &self,
        fields: &Value,
    ) -> AgentSessionFuture<'static, Value> {
        let shared = self.shared.clone();
        let fields = fields.clone();
        let models = self.models.clone();
        Box::pin(async move {
            let operation = fields["operation"]
                .as_str()
                .ok_or(AgentSessionError::Rejected)?;
            let result = operate(shared, &models, operation, &fields).await;
            Ok(match result {
                Ok(details) => json!({"text":details.to_string(),"error":null}),
                Err(error) => json!({"text":"","error":error.to_string()}),
            })
        })
    }

    /// Route only owned host tools; every other host request retains the existing rejection path.
    pub(in crate::local::omp) fn tool(
        &mut self,
        event: &Value,
        writer: Writer,
    ) -> Result<bool, AgentSessionError> {
        self.calls.retain(|_, task| !task.is_finished());
        if event["type"] == "host_tool_cancel" {
            return Ok(event["targetId"]
                .as_str()
                .is_some_and(|id| self.calls.remove(id).is_some()));
        }
        let Some(operation) = event["toolName"]
            .as_str()
            .and_then(|name| name.strip_prefix("vibe_"))
        else {
            return Ok(false);
        };
        let id = event["id"]
            .as_str()
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        if self.calls.contains_key(&id) {
            return Err(AgentSessionError::Rejected);
        }
        if self.calls.len() >= MAX_CALLS {
            return Err(AgentSessionError::Busy);
        }
        let operation = operation.to_owned();
        let fields = event["arguments"].clone();
        let shared = self.shared.clone();
        let models = self.models.clone();
        let reply_id = id.clone();
        let task = tokio::spawn(async move {
            let result = operate(shared, &models, &operation, &fields).await;
            let (details, error) = match result {
                Ok(value) => (value, false),
                Err(error) => (json!({"error": error.to_string()}), true),
            };
            let result = tools::result(&details, error);
            let frame = json!({"type":"host_tool_result", "id":reply_id,
                "isError":result["isError"], "result":result});
            let _ = writer.send(&frame).await;
        });
        self.calls.insert(id, AbortOnDropHandle::new(task));
        Ok(true)
    }

    /// Consume worker events and publish them only in the child's own timeline.
    pub(in crate::local::omp) fn poll(&mut self, writer: Writer) -> Vec<AgentTurnEvent> {
        let mut events = std::mem::take(&mut self.lifecycle_events);
        for _ in 0..128 {
            let Ok((generation, event)) = self.incoming.try_recv() else {
                break;
            };
            if generation != self.shared.generation.load(Ordering::Acquire) {
                continue;
            }
            match event {
                HostEvent::Record(record) => {
                    let record = *record;
                    let terminal = record.child.descriptor["status"] != "running";
                    if terminal && self.enabled() {
                        self.notifications
                            .retain(|value| value["id"] != record.child.id);
                        self.notifications.push(record.view());
                    }
                    if terminal {
                        let stale: Vec<_> = self
                            .permissions
                            .iter()
                            .filter(|(_, (worker, _, _))| worker == &record.child.id)
                            .map(|(id, _)| id.clone())
                            .collect();
                        for id in stale {
                            self.permissions.remove(&id);
                            events.push(AgentTurnEvent::PermissionResolved(id));
                        }
                    }
                    if let Ok(mut registry) = self.shared.registry.lock() {
                        registry
                            .records
                            .insert(record.child.id.clone(), record.clone());
                    }
                    self.shared
                        .changes
                        .send_modify(|value| *value = value.wrapping_add(1));
                    events.push(AgentTurnEvent::Subagent(SubagentEvent::Upsert(
                        record.child,
                    )));
                }
                HostEvent::Event(event) => events.push(event),
                HostEvent::Permission {
                    host,
                    worker,
                    native,
                    request,
                } => {
                    if self.permissions.len() < 128 {
                        self.permissions
                            .insert(host, (worker, native, request.clone()));
                        events.push(AgentTurnEvent::PermissionRequested(request));
                    }
                }
            }
        }
        if !self.notifications.is_empty()
            && self
                .delivery
                .as_ref()
                .is_none_or(AbortOnDropHandle::is_finished)
        {
            let result = self.notifications.remove(0);
            self.delivery = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                let message = format!("/ait-vibe-result-v1 {}", json!({"workers":[result]}));
                let _ = writer.send(&json!({"type":"prompt","id":uuid::Uuid::new_v4().to_string(),"message":message})).await;
            })));
        }
        events
    }

    /// Stop all worker writers and in-flight tool jobs on operator cancellation or mode exit.
    pub(in crate::local::omp) fn stop(&mut self) {
        self.shared.generation.fetch_add(1, Ordering::AcqRel);
        self.calls.clear();
        self.notifications.clear();
        self.delivery.take();
        self.lifecycle_events.extend(
            self.permissions
                .keys()
                .cloned()
                .map(AgentTurnEvent::PermissionResolved),
        );
        self.permissions.clear();
        while self.incoming.try_recv().is_ok() {}
        if let Ok(mut registry) = self.shared.registry.lock() {
            registry.slots.clear();
            registry.starting.clear();
            for record in registry.records.values_mut() {
                if record.child.descriptor["status"] == "running" {
                    record.status("canceled");
                    self.lifecycle_events
                        .push(AgentTurnEvent::Subagent(SubagentEvent::Upsert(
                            record.child.clone(),
                        )));
                }
            }
        }
        self.shared
            .changes
            .send_modify(|value| *value = value.wrapping_add(1));
    }

    /// Reject configuration and history changes while workers own active turns or are starting.
    pub(in crate::local::omp) fn running(&self) -> bool {
        self.shared.registry.lock().map_or(true, |registry| {
            !registry.starting.is_empty()
                || registry
                    .records
                    .values()
                    .any(|record| record.child.descriptor["status"] == "running")
        })
    }

    /// Return current worker approvals in the parent's existing approval channel.
    pub(in crate::local::omp) fn pending(&self) -> Vec<Value> {
        self.permissions
            .values()
            .map(|(_, _, request)| request.clone())
            .collect()
    }
    /// Check ownership before accepting a worker approval response.
    pub(in crate::local::omp) fn owns_permission(&self, id: &str) -> bool {
        self.permissions.contains_key(id)
    }
    /// Route an owned approval response to its sole worker writer; reject stale requests.
    pub(in crate::local::omp) async fn permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        let (worker, native, _) = self
            .permissions
            .get(id)
            .ok_or(AgentSessionError::Rejected)?;
        let sender = self
            .shared
            .registry
            .lock()
            .map_err(|_| AgentSessionError::Failed)?
            .slots
            .get(worker)
            .map(|slot| slot.commands.clone())
            .ok_or(AgentSessionError::Rejected)?;
        let (reply, ready) = oneshot::channel();
        sender
            .send(worker::Command::Permission(
                native.clone(),
                response.clone(),
                reply,
            ))
            .await
            .map_err(|_| AgentSessionError::Failed)?;
        ready.await.map_err(|_| AgentSessionError::Failed)??;
        self.permissions.remove(id);
        Ok(())
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.shared.enabled.store(false, Ordering::Release);
        self.stop();
    }
}

async fn activate(
    transport: &mut Transport,
) -> Result<BTreeMap<String, String>, AgentSessionError> {
    let registered = transport
        .request("set_host_tools", json!({"tools":tools::definitions()}))
        .await?;
    if !registered["toolNames"].as_array().is_some_and(|names| {
        tools::definitions()
            .iter()
            .all(|tool| names.contains(&tool["name"]))
    }) {
        return Err(AgentSessionError::Unavailable);
    }
    let events = transport.local_command(&format!("/{COMMAND} on")).await?;
    let routes = events
        .iter()
        .filter(|event| event["type"] == "extension_ui_request" && event["method"] == "notify")
        .filter_map(|event| event["message"].as_str()?.strip_prefix("AIT_VIBE_V1:"))
        .find_map(|text| serde_json::from_str::<Value>(text).ok())
        .ok_or(AgentSessionError::Unavailable)?;
    let mut models = BTreeMap::new();
    for role in ["fast", "good"] {
        let route: [String; 2] = serde_json::from_value(routes[role].clone())
            .map_err(|_| AgentSessionError::Unavailable)?;
        if route.iter().any(String::is_empty) {
            return Err(AgentSessionError::Unavailable);
        }
        models.insert(
            role.to_owned(),
            serde_json::to_string(&route).map_err(|_| AgentSessionError::Failed)?,
        );
    }
    Ok(models)
}

fn text<'a>(fields: &'a Value, key: &str, max: usize) -> Result<&'a str, AgentSessionError> {
    fields[key]
        .as_str()
        .filter(|text| !text.trim().is_empty() && text.len() <= max)
        .ok_or(AgentSessionError::Rejected)
}

async fn operate(
    shared: Arc<Shared>,
    models: &BTreeMap<String, String>,
    operation: &str,
    fields: &Value,
) -> Result<Value, AgentSessionError> {
    if !shared.enabled.load(Ordering::Acquire) {
        return Err(AgentSessionError::Rejected);
    }
    match operation {
        "list" => {
            let registry = shared
                .registry
                .lock()
                .map_err(|_| AgentSessionError::Failed)?;
            Ok(json!({"workers":registry.records.values().map(Record::view).collect::<Vec<_>>()}))
        }
        "spawn" => spawn(shared, models, fields).await,
        "send" | "kill" => {
            let id = fields["sessionId"]
                .as_str()
                .or_else(|| fields["session"].as_str())
                .ok_or(AgentSessionError::Rejected)?;
            let (sender, record) = {
                let registry = shared
                    .registry
                    .lock()
                    .map_err(|_| AgentSessionError::Failed)?;
                (
                    registry.slots.get(id).map(|slot| slot.commands.clone()),
                    registry
                        .records
                        .get(id)
                        .cloned()
                        .ok_or(AgentSessionError::Rejected)?,
                )
            };
            if operation == "send" {
                let message = text(fields, "message", 65536)?.to_owned();
                if let Some(sender) = sender {
                    let (reply, ready) = oneshot::channel();
                    sender
                        .send(worker::Command::Send(message, reply))
                        .await
                        .map_err(|_| AgentSessionError::Failed)?;
                    ready.await.map_err(|_| AgentSessionError::Failed)??;
                } else {
                    launch(
                        shared.clone(),
                        WorkerRequest {
                            cli: record.cli.clone(),
                            name: record.child.descriptor["description"]
                                .as_str()
                                .unwrap_or("Worker")
                                .to_owned(),
                            prompt: message,
                            config: record.config,
                            restore: record.child.persistence,
                        },
                    )
                    .await?;
                }
            } else if let Some(sender) = sender {
                let (reply, ready) = oneshot::channel();
                sender
                    .send(worker::Command::Kill(reply))
                    .await
                    .map_err(|_| AgentSessionError::Failed)?;
                tokio::time::timeout(Duration::from_secs(10), ready)
                    .await
                    .map_err(|_| AgentSessionError::Failed)?
                    .map_err(|_| AgentSessionError::Failed)?;
                let mut registry = shared
                    .registry
                    .lock()
                    .map_err(|_| AgentSessionError::Failed)?;
                registry.slots.remove(id);
            }
            Ok(json!({"session":id,"accepted":true}))
        }
        "wait" => wait(shared, fields).await,
        _ => Err(AgentSessionError::Rejected),
    }
}

async fn spawn(
    shared: Arc<Shared>,
    models: &BTreeMap<String, String>,
    fields: &Value,
) -> Result<Value, AgentSessionError> {
    let cli = text(fields, "cli", 8)?.to_owned();
    let model = models.get(&cli).ok_or(AgentSessionError::Rejected)?.clone();
    let prompt = text(fields, "prompt", 65536)?.to_owned();
    let name = fields
        .get("name")
        .map(|_| text(fields, "name", 48))
        .transpose()?
        .unwrap_or(&cli)
        .to_owned();
    let mut config = shared.spec.config.clone();
    if let Some(values) = &mut config.feature_values {
        values.remove("modelPresetThinking");
    }
    config.thinking_option_id = shared
        .client
        .cached_roles(&shared.spec.cwd)?
        .and_then(|catalog| {
            catalog.worker_thinking(&config, if cli == "fast" { "smol" } else { "task" })
        });
    config.model = Some(model);
    launch(
        shared,
        WorkerRequest {
            cli,
            name,
            prompt,
            config,
            restore: None,
        },
    )
    .await
}

#[derive(Debug)]
struct WorkerRequest {
    cli: String,
    name: String,
    prompt: String,
    config: StoredAgentConfig,
    restore: Option<AgentPersistenceHandle>,
}

struct Reservation {
    shared: Arc<Shared>,
    key: String,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.shared.registry.lock() {
            registry.starting.remove(&self.key);
        }
    }
}

async fn launch(shared: Arc<Shared>, request: WorkerRequest) -> Result<Value, AgentSessionError> {
    let generation = shared.generation.load(Ordering::Acquire);
    let key = request.restore.as_ref().map_or_else(
        || uuid::Uuid::new_v4().to_string(),
        |handle| handle.session_id.clone(),
    );
    {
        let mut registry = shared
            .registry
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        if registry.starting.contains_key(&key)
            || registry.slots.contains_key(&key)
            || (request.restore.is_none()
                && registry.records.len() + registry.starting.len() >= MAX_WORKERS)
        {
            return Err(AgentSessionError::Busy);
        }
        registry.starting.insert(key.clone(), None);
    }
    let reservation = Reservation {
        shared: shared.clone(),
        key,
    };
    let mut client = shared.client.clone();
    client.vibe_host = false;
    let spec = AgentSessionSpec {
        config: request.config,
        ..shared.spec.clone()
    };
    let launch = Launch {
        generation,
        client,
        spec,
        parent: shared.parent.clone(),
        parent_file: shared.parent_file.clone(),
        cli: request.cli,
        name: request.name,
        prompt: request.prompt,
        restore: request.restore,
    };
    let (sender, commands) = mpsc::channel(16);
    let (reply, ready) = oneshot::channel();
    let task = AbortOnDropHandle::new(tokio::spawn(worker::run(
        launch,
        commands,
        shared.events.clone(),
        reply,
    )));
    {
        let mut registry = shared
            .registry
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        let slot = registry
            .starting
            .get_mut(&reservation.key)
            .ok_or(AgentSessionError::Rejected)?;
        *slot = Some(task);
    }
    let result = ready.await.map_err(|_| AgentSessionError::Failed)?;
    let mut registry = shared
        .registry
        .lock()
        .map_err(|_| AgentSessionError::Failed)?;
    let record = result?;
    if !shared.enabled.load(Ordering::Acquire)
        || shared.generation.load(Ordering::Acquire) != generation
    {
        return Err(AgentSessionError::Rejected);
    }
    let task = registry
        .starting
        .remove(&reservation.key)
        .flatten()
        .ok_or(AgentSessionError::Rejected)?;
    let id = record.child.id.clone();
    registry.records.entry(id.clone()).or_insert(record.clone());
    registry.slots.insert(
        id.clone(),
        Slot {
            commands: sender,
            _task: task,
        },
    );
    drop(registry);
    drop(reservation);
    Ok(json!({"session":id,"status":"running"}))
}

async fn wait(shared: Arc<Shared>, fields: &Value) -> Result<Value, AgentSessionError> {
    let timeout = fields
        .get("timeout")
        .map_or(Some(30.0), Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0 && *value <= 600.0)
        .ok_or(AgentSessionError::Rejected)?;
    let sessions = fields
        .get("sessions")
        .map(|value| serde_json::from_value::<Vec<String>>(value.clone()))
        .transpose()
        .map_err(|_| AgentSessionError::Rejected)?;
    let mut changes = shared.changes.subscribe();
    let sessions = if let Some(ids) = sessions {
        let registry = shared
            .registry
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        if ids.len() > MAX_WORKERS || ids.iter().any(|id| !registry.records.contains_key(id)) {
            return Err(AgentSessionError::Rejected);
        }
        ids
    } else {
        let registry = shared
            .registry
            .lock()
            .map_err(|_| AgentSessionError::Failed)?;
        let running: Vec<_> = registry
            .records
            .values()
            .filter(|record| record.child.descriptor["status"] == "running")
            .map(|record| record.child.id.clone())
            .collect();
        if running.is_empty() {
            registry.records.keys().cloned().collect()
        } else {
            running
        }
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs_f64(timeout);
    loop {
        if !shared.enabled.load(Ordering::Acquire) {
            return Err(AgentSessionError::Rejected);
        }
        let records = {
            let registry = shared
                .registry
                .lock()
                .map_err(|_| AgentSessionError::Failed)?;
            registry
                .records
                .values()
                .filter(|record| sessions.contains(&record.child.id))
                .cloned()
                .collect::<Vec<_>>()
        };
        if records.is_empty()
            || records
                .iter()
                .any(|record| record.child.descriptor["status"] != "running")
            || tokio::time::Instant::now() >= deadline
        {
            return Ok(json!({"workers":records.iter().map(Record::view).collect::<Vec<_>>()}));
        }
        if tokio::time::timeout_at(deadline, changes.changed())
            .await
            .is_err()
        {
            return Ok(json!({"workers":records.iter().map(Record::view).collect::<Vec<_>>()}));
        }
    }
}

/// Hide private bridge commands and advertise Vibe only after the public extension was loaded.
pub(in crate::local::omp) fn visible_commands(native: &[Value], available: bool) -> Vec<Value> {
    let mut commands = super::super::commands::visible(native);
    commands.retain(|entry| {
        !entry["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("ait-vibe-"))
    });
    if available {
        commands.push(json!({"name":"vibe","source":"extension","description":"Ait Vibe director using external OMP fast/good workers","argumentHint":"[instruction]","kind":"command"}));
    }
    commands
}

#[cfg(test)]
mod tests;
