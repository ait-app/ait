use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::{Semaphore, mpsc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

mod mutation;

struct Lane {
    sender: mpsc::Sender<super::lane::Message>,
    barriers: Vec<CancellationToken>,
}

use super::operation::{Access, Operation};
use super::{Command, ErrorCode, ExecutionState};

/// Shared factories and independently owned session lanes; all accepted work is tracked.
pub(super) struct Router {
    template: Arc<Mutex<ExecutionState>>,
    lanes: BTreeMap<String, Lane>,
    tasks: TaskTracker,
    reads: Arc<Semaphore>,
    providers: Arc<Semaphore>,
    retirements: Arc<Semaphore>,
}

impl Router {
    pub(super) fn new(template: Arc<Mutex<ExecutionState>>) -> Self {
        Self {
            template,
            lanes: BTreeMap::new(),
            tasks: TaskTracker::new(),
            reads: Arc::new(Semaphore::new(16)),
            providers: Arc::new(Semaphore::new(16)),
            retirements: Arc::new(Semaphore::new(32)),
        }
    }

    pub(super) async fn dispatch(&mut self, command: Command) {
        let Command::Request {
            method,
            mut params,
            reply,
            cancel,
            realtime,
            permit,
            queued,
        } = command
        else {
            return;
        };
        let template = self.template.clone();
        let planned = tokio::task::spawn_blocking(move || {
            let state = template.lock().map_err(|_| ErrorCode::AgentIo)?.fork(None);
            let scopes = if mutation::is_mutation(method) {
                mutation::scopes(&state, method, &params)?
            } else {
                BTreeSet::new()
            };
            canonicalize(&state, method, &mut params)?;
            plan(state, method, &params).map(|(state, lane)| (state, lane, scopes, method, params))
        })
        .await;
        let (state, lane, scopes, method, params) = match planned {
            Ok(Ok(plan)) => plan,
            Ok(Err(error)) => {
                let _ = reply.send(Err(error));
                return;
            }
            Err(_) => {
                let _ = reply.send(Err(ErrorCode::AgentIo));
                return;
            }
        };
        let command = Command::Request {
            method,
            params,
            reply,
            cancel,
            realtime,
            permit,
            queued,
        };
        if mutation::is_mutation(method) {
            self.mutate(state, command, scopes);
            return;
        }
        if let Some(key) = lane {
            self.lanes.retain(|_, lane| !lane.sender.is_closed());
            if !self.lanes.contains_key(&key) && self.lanes.len() >= 128 {
                reject(command, ErrorCode::CatalogBusy);
                return;
            }
            let barriers = match state.owners.startup(&key) {
                Ok(barriers) => barriers,
                Err(error) => {
                    reject(command, error);
                    return;
                }
            };
            let sender = self.lanes.entry(key).or_insert_with(|| {
                let (sender, receiver) = mpsc::channel(64);
                self.tasks
                    .spawn(super::lane::serve(state, receiver, barriers.clone()));
                Lane { sender, barriers }
            });
            if let Err(error) = sender
                .sender
                .try_send(super::lane::Message::Request(command))
                && let super::lane::Message::Request(command) = error.into_inner()
            {
                reject(command, ErrorCode::CatalogBusy);
            }
        } else {
            let reads = if matches!(method.access(), Access::Catalog | Access::Discovery)
                && !matches!(method, Operation::Snapshot | Operation::RefreshSnapshot)
            {
                self.providers.clone()
            } else {
                self.reads.clone()
            };
            self.tasks.spawn(async move {
                let _permit = reads.acquire_owned().await;
                let _ = super::lane::request(state, command, false).await;
            });
        }
    }

    pub(super) async fn maintenance(&mut self) {
        let template = self.template.clone();
        let handle = tokio::runtime::Handle::current();
        let queued = tokio::task::spawn_blocking(move || {
            let mut state = template.lock().map_err(|_| ErrorCode::AgentIo)?;
            handle.block_on(state.manager.poll_generated_titles())?;
            state
                .manager
                .timeline()
                .map(|timeline| timeline.queued_agents())
                .transpose()
                .map(Option::unwrap_or_default)
        })
        .await;
        let Ok(Ok(ids)) = queued else {
            return;
        };
        for id in ids {
            let prepared = {
                let template = self.template.clone();
                tokio::task::spawn_blocking(move || {
                    let state = template.lock().map_err(|_| ErrorCode::AgentIo)?.fork(None);
                    plan(state, Operation::Wake, &serde_json::json!({"agentId":id}))
                })
                .await
            };
            let Ok(Ok((state, Some(key)))) = prepared else {
                continue;
            };
            self.lanes.retain(|_, lane| !lane.sender.is_closed());
            if self.lanes.len() >= 128 && !self.lanes.contains_key(&key) {
                continue;
            }
            let Ok(barriers) = state.owners.startup(&key) else {
                continue;
            };
            self.lanes.entry(key).or_insert_with(|| {
                let (sender, receiver) = mpsc::channel(64);
                self.tasks
                    .spawn(super::lane::serve(state, receiver, barriers.clone()));
                Lane { sender, barriers }
            });
        }
    }

    pub(super) async fn close(self) -> Result<(), ErrorCode> {
        let catalog = self
            .template
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .manager
            .catalog();
        let mut completions = Vec::with_capacity(self.lanes.len());
        for sender in self.lanes.values() {
            let (reply, result) = tokio::sync::oneshot::channel();
            if sender
                .sender
                .send(super::lane::Message::Shutdown(reply))
                .await
                .is_ok()
            {
                completions.push(result);
            }
        }
        let mut failure = None;
        for result in completions {
            if let Err(error) = result.await.unwrap_or(Err(ErrorCode::AgentIo)) {
                failure.get_or_insert(error);
            }
        }
        self.tasks.close();
        self.tasks.wait().await;
        catalog.close().await;
        let template = self.template;
        let handle = tokio::runtime::Handle::current();
        let closed = tokio::task::spawn_blocking(move || {
            let mut state = template.lock().map_err(|_| ErrorCode::AgentIo)?;
            handle
                .block_on(state.manager.close_all())
                .map_err(|_| ErrorCode::AgentIo)
        })
        .await
        .map_err(|_| ErrorCode::AgentIo)?;
        closed?;
        failure.map_or(Ok(()), Err)
    }
}

fn plan(
    mut state: ExecutionState,
    method: Operation,
    params: &Value,
) -> Result<(ExecutionState, Option<String>), ErrorCode> {
    let lane = if mutation::is_mutation(method) {
        None
    } else if method == Operation::Bot {
        let id = params["operation"]["agent_id"]
            .as_str()
            .ok_or(ErrorCode::InvalidMessage)?;
        let lane = state.owners.agent(id)?;
        if let Some(record) = state.registry.get(id).map_err(|_| ErrorCode::AgentIo)? {
            state.owners.bind(&lane, &record)?;
        }
        if let Some(workspace) = params["owner"]["workspaceId"].as_str() {
            state
                .owners
                .place(&lane, format!("workspace:{workspace}"))?;
        }
        Some(lane)
    } else if method == Operation::Create {
        if let Some(id) = params["agentId"].as_str() {
            Some(state.owners.agent(id)?)
        } else {
            Some(format!(
                "creation:{}",
                params["idempotencyKey"]
                    .as_str()
                    .ok_or(ErrorCode::InvalidMessage)?
            ))
        }
    } else if method == Operation::WorkspaceCreate {
        Some(
            state.owners.agent(
                params["creation"]["agentId"]
                    .as_str()
                    .ok_or(ErrorCode::InvalidMessage)?,
            )?,
        )
    } else if matches!(method, Operation::Resume | Operation::Import) {
        native_lane(&state, method, params)?
    } else if matches!(
        method.access(),
        Access::Read | Access::Catalog | Access::Discovery
    ) && !(method == Operation::Usage && params.get("agentId").is_some())
    {
        None
    } else if let Some(identifier) = method
        .target()
        .and_then(|target| params.pointer(target.pointer()))
        .and_then(Value::as_str)
    {
        let id = state.resolve(identifier)?;
        if method.access() == Access::Timeline
            && (state.manager.history_loaded(&id)
                || state.can_read_cached_timeline(method.as_str(), params)?)
        {
            None
        } else {
            let lane = state.owners.agent(&id)?;
            if let Some(record) = state.registry.get(&id).map_err(|_| ErrorCode::AgentIo)? {
                state.owners.bind(&lane, &record)?;
            }
            Some(lane)
        }
    } else {
        return Err(ErrorCode::InvalidMessage);
    };
    if let Some(lane) = &lane {
        if method == Operation::Create
            && let Some(cwd) = params["config"]["cwd"].as_str()
            && let Ok(workspace) = state.workspace(None, cwd)
        {
            state.owners.place(lane, format!("workspace:{workspace}"))?;
        }
        if let Some(workspace) = params["workspaceId"]
            .as_str()
            .or(params["creation"]["workspaceId"].as_str())
        {
            state.owners.place(lane, format!("workspace:{workspace}"))?;
        }
        if let Some(caller) = params["callerAgentId"].as_str() {
            state.owners.place(lane, format!("parent:{caller}"))?;
            if let Some(record) = state.registry.get(caller).map_err(|_| ErrorCode::AgentIo)?
                && let Some(workspace) = record.workspace_id
            {
                state.owners.place(lane, format!("workspace:{workspace}"))?;
            }
        }
        state.manager = state.manager.fork(Some(state.owners.owner(lane.clone())));
    }
    Ok((state, lane))
}

fn native_lane(
    state: &ExecutionState,
    method: Operation,
    params: &Value,
) -> Result<Option<String>, ErrorCode> {
    let (provider, session) = if method == Operation::Resume {
        (
            params["handle"]["provider"].as_str(),
            params["handle"]["sessionId"].as_str(),
        )
    } else {
        (
            params["providerId"]
                .as_str()
                .or(params["provider"].as_str()),
            params["providerHandleId"]
                .as_str()
                .or(params["sessionId"].as_str()),
        )
    };
    let (provider, session) = provider.zip(session).ok_or(ErrorCode::InvalidMessage)?;
    let records = state.registry.list().map_err(|_| ErrorCode::AgentIo)?;
    let mut matches = records.iter().filter(|record| {
        record.persistence.as_ref().is_some_and(|handle| {
            handle.provider == provider
                && (handle.session_id == session
                    || handle.native_handle.as_ref().and_then(Value::as_str) == Some(session))
        })
    });
    let record = matches.next();
    if matches.next().is_some() {
        return Err(ErrorCode::InvalidMessage);
    }
    if let Some(record) = record {
        if record.internal {
            return Err(ErrorCode::AgentNotFound);
        }
        let lane = state.owners.agent(&record.id)?;
        state.owners.bind(&lane, record)?;
        Ok(Some(lane))
    } else {
        state.owners.native(provider, session).map(Some)
    }
}

fn canonicalize(
    state: &ExecutionState,
    method: Operation,
    params: &mut Value,
) -> Result<(), ErrorCode> {
    if method == Operation::Commands && params.get("draftConfig").is_some() {
        return Ok(());
    }
    if let Some(target) = method.target()
        && let Some(identifier) = params.pointer(target.pointer()).and_then(Value::as_str)
    {
        let id = match state.resolve(identifier) {
            Ok(id) => id,
            Err(ErrorCode::AgentNotFound) if method == Operation::Get => return Ok(()),
            Err(error) => return Err(error),
        };
        *params
            .pointer_mut(target.pointer())
            .ok_or(ErrorCode::InvalidMessage)? = Value::String(id);
    }
    Ok(())
}

fn reject(command: Command, error: ErrorCode) {
    if let Command::Request { reply, .. } = command {
        let _ = reply.send(Err(error));
    }
}
