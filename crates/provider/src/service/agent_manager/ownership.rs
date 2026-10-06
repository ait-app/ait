//! Stable routing aliases and committed observations shared by independent session owners.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::rpc::ErrorCode;
use domain::agent_runtime::PersistedAgentRuntimeRecord;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::auto_archive::Retirement;

#[derive(Debug)]
struct PendingRetirement {
    action: Retirement,
    running: bool,
}

#[derive(Debug, Default)]
struct Routes {
    agents: BTreeMap<String, String>,
    native: BTreeMap<(String, String), String>,
    scopes: BTreeMap<String, BTreeSet<String>>,
}

/// Routing and post-commit observations; these locks never cover native I/O.
#[derive(Debug, Clone, Default)]
pub(crate) struct Owners {
    routes: Arc<Mutex<Routes>>,
    observations: Arc<Mutex<BTreeMap<String, watch::Sender<Value>>>>,
    barriers: Arc<Mutex<BTreeMap<String, Vec<CancellationToken>>>>,
    retirements: Arc<Mutex<BTreeMap<String, PendingRetirement>>>,
    changes: Option<model::changes::Changes>,
}

/// A metadata mutation fences existing lanes and delays newly admitted related lanes.
#[derive(Debug)]
pub(crate) struct Barrier {
    shared: Owners,
    scopes: Vec<String>,
    release: CancellationToken,
}

impl Drop for Barrier {
    fn drop(&mut self) {
        self.release.cancel();
        if let Ok(mut barriers) = self.shared.barriers.lock() {
            for scope in &self.scopes {
                if let Some(tokens) = barriers.get_mut(scope) {
                    tokens.retain(|token| !token.is_cancelled());
                    if tokens.is_empty() {
                        barriers.remove(scope);
                    }
                }
            }
        }
    }
}

/// One lane's exclusive session ownership and input scope.
#[derive(Debug, Clone)]
pub(crate) struct Owner {
    shared: Owners,
    lane: String,
}

impl Owners {
    /// Wake directory observers after the runtime overlay becomes coherently visible.
    pub(crate) fn with_changes(mut self, changes: Option<model::changes::Changes>) -> Self {
        self.changes = changes;
        self
    }

    /// Take unstarted automatic retirements; failed actions remain retryable.
    /// # Errors
    /// Returns Agent I/O errors if retirement state is poisoned.
    pub(crate) fn retirements(&self) -> Result<Vec<Retirement>, ErrorCode> {
        let mut retirements = self.retirements.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok(retirements
            .values_mut()
            .filter_map(|pending| {
                if pending.running {
                    return None;
                }
                pending.running = true;
                Some(pending.action.clone())
            })
            .collect())
    }

    /// Complete a retirement or make its close/cleanup failure eligible for retry.
    pub(crate) fn finish_retirement(&self, id: &str, success: bool) {
        if let Ok(mut retirements) = self.retirements.lock() {
            if success {
                retirements.remove(id);
            } else if let Some(pending) = retirements.get_mut(id) {
                pending.running = false;
            }
        }
        if success
            && let Ok(observations) = self.observations.lock()
            && let Some(sender) = observations.get(id)
        {
            sender.send_modify(|value| value["busy"] = json!(false));
        }
    }

    /// Keep completion pending until native close and automatic worktree cleanup finish.
    pub(crate) fn retiring(&self, id: &str) -> bool {
        self.retirements
            .lock()
            .is_ok_and(|retirements| retirements.contains_key(id))
    }

    /// Register placement before native creation can wait on external I/O.
    /// # Errors
    /// Returns Agent I/O errors if routing state is poisoned.
    pub(crate) fn place(&self, lane: &str, scope: String) -> Result<(), ErrorCode> {
        self.routes
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .scopes
            .entry(lane.to_owned())
            .or_default()
            .insert(scope);
        Ok(())
    }

    /// Return existing lanes affected by a Workspace or parent lifecycle mutation.
    /// # Errors
    /// Returns Agent I/O errors if routing state is poisoned.
    pub(crate) fn related(&self, scopes: &BTreeSet<String>) -> Result<BTreeSet<String>, ErrorCode> {
        let routes = self.routes.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok(routes
            .scopes
            .iter()
            .filter(|(_, placed)| !placed.is_disjoint(scopes))
            .map(|(lane, _)| lane.clone())
            .collect())
    }

    /// Prevent new related lanes from executing until the durable mutation and closes finish.
    /// # Errors
    /// Returns Agent I/O errors if barrier state is poisoned.
    pub(crate) fn freeze(&self, scopes: Vec<String>) -> Result<Barrier, ErrorCode> {
        let release = CancellationToken::new();
        let mut barriers = self.barriers.lock().map_err(|_| ErrorCode::AgentIo)?;
        for scope in &scopes {
            barriers
                .entry(scope.clone())
                .or_default()
                .push(release.clone());
        }
        Ok(Barrier {
            shared: self.clone(),
            scopes,
            release,
        })
    }

    /// Capture startup barriers at admission, before later metadata operations can fence the lane.
    /// # Errors
    /// Returns Agent I/O errors if routing or barrier state is poisoned.
    pub(crate) fn startup(&self, lane: &str) -> Result<Vec<CancellationToken>, ErrorCode> {
        let mut scopes = self
            .routes
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .scopes
            .get(lane)
            .cloned()
            .unwrap_or_default();
        scopes.insert(format!("lane:{lane}"));
        let barriers = self.barriers.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok(scopes
            .iter()
            .filter_map(|scope| barriers.get(scope))
            .flatten()
            .cloned()
            .collect())
    }

    /// Release inactive aliases after a lane has reaped all of its sessions.
    pub(crate) fn release(&self, lane: &str) {
        if let Ok(mut routes) = self.routes.lock() {
            routes.agents.retain(|_, owner| owner != lane);
            routes.native.retain(|_, owner| owner != lane);
            routes.scopes.remove(lane);
        }
    }
    /// Fence completion reads while a queued writer operation is being admitted.
    /// # Errors
    /// Returns Agent I/O errors if the observation registry is poisoned.
    pub(crate) fn pending(&self, ids: &[String]) -> Result<(), ErrorCode> {
        let mut observations = self.observations.lock().map_err(|_| ErrorCode::AgentIo)?;
        for id in ids {
            let sender = observations.entry(id.clone()).or_insert_with(|| watch::channel(
                json!({"status":"running","final":null,"error":null,"lastMessage":null,"live":false,"busy":true})
            ).0);
            sender.send_if_modified(|value| {
                if value["busy"] == true {
                    return false;
                }
                value["busy"] = json!(true);
                true
            });
        }
        Ok(())
    }
    /// Resolve an Agent to its existing owner, including imported native aliases.
    /// # Errors
    /// Returns Agent I/O errors if shared routing state is poisoned.
    pub(crate) fn agent(&self, id: &str) -> Result<String, ErrorCode> {
        let mut routes = self.routes.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok(routes
            .agents
            .entry(id.to_owned())
            .or_insert_with(|| format!("agent:{id}"))
            .clone())
    }

    /// Serialize unknown imports and resumes by their provider-owned identity.
    /// # Errors
    /// Returns Agent I/O errors if shared routing state is poisoned.
    pub(crate) fn native(&self, provider: &str, session: &str) -> Result<String, ErrorCode> {
        let mut routes = self.routes.lock().map_err(|_| ErrorCode::AgentIo)?;
        let key = (provider.to_owned(), session.to_owned());
        Ok(routes
            .native
            .entry(key)
            .or_insert_with(|| format!("native:{}", json!([provider, session])))
            .clone())
    }

    /// Bind a runtime record before publishing its durable registration.
    /// # Errors
    /// Rejects an identity already owned by another lane.
    pub(crate) fn bind(
        &self,
        lane: &str,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), ErrorCode> {
        let mut routes = self.routes.lock().map_err(|_| ErrorCode::AgentIo)?;
        let mut native = Vec::new();
        if let Some(handle) = &record.persistence {
            native.push((handle.provider.clone(), handle.session_id.clone()));
            if let Some(alias) = handle.native_handle.as_ref().and_then(Value::as_str) {
                native.push((handle.provider.clone(), alias.to_owned()));
            }
        }
        if routes
            .agents
            .get(&record.id)
            .is_some_and(|owner| owner != lane)
            || native
                .iter()
                .any(|key| routes.native.get(key).is_some_and(|owner| owner != lane))
        {
            return Err(ErrorCode::CatalogBusy);
        }
        routes.agents.insert(record.id.clone(), lane.to_owned());
        for key in native {
            routes.native.insert(key, lane.to_owned());
        }
        let scopes = routes.scopes.entry(lane.to_owned()).or_default();
        if let Some(workspace) = &record.workspace_id {
            scopes.insert(format!("workspace:{workspace}"));
        }
        if let Some(parent) = record.labels.get("paseo.parent-agent-id") {
            scopes.insert(format!("parent:{parent}"));
        }
        Ok(())
    }

    /// Construct the exclusive owner attached to one lane's manager.
    pub(crate) fn owner(&self, lane: String) -> Owner {
        Owner {
            shared: self.clone(),
            lane,
        }
    }

    /// Observe a committed completion snapshot without submitting a command.
    /// # Errors
    /// Returns Agent I/O errors if the observation registry is poisoned.
    pub(crate) fn observe(
        &self,
        id: &str,
        initial: Value,
    ) -> Result<watch::Receiver<Value>, ErrorCode> {
        let mut observations = self.observations.lock().map_err(|_| ErrorCode::AgentIo)?;
        Ok(observations
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(initial).0)
            .subscribe())
    }

    /// Publish only a changed, fully committed snapshot and wake its waiters.
    /// # Errors
    /// Returns Agent I/O errors if the observation registry is poisoned.
    pub(crate) fn publish(&self, id: &str, value: Value) -> Result<(), ErrorCode> {
        let mut observations = self.observations.lock().map_err(|_| ErrorCode::AgentIo)?;
        let sender = observations
            .entry(id.to_owned())
            .or_insert_with(|| watch::channel(Value::Null).0);
        let snapshot_changed = sender.borrow()["final"] != value["final"];
        let changed = sender.send_if_modified(|current| {
            if *current == value {
                return false;
            }
            *current = value;
            true
        });
        if changed
            && snapshot_changed
            && let Some(changes) = &self.changes
        {
            changes.notify();
        }
        Ok(())
    }

    /// Return the last committed runtime overlay for an independent reader.
    pub(crate) fn snapshot(&self, id: &str) -> Option<Value> {
        let observations = self.observations.lock().ok()?;
        observations.get(id).and_then(|sender| {
            sender
                .borrow()
                .get("final")
                .filter(|value| value.is_object())
                .cloned()
        })
    }
}

impl Owner {
    /// Queue automatic retirement without closing another owner's native resources.
    pub(super) fn retire(&self, action: Retirement) -> Result<(), ErrorCode> {
        self.shared
            .retirements
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .entry(action.id.clone())
            .or_insert(PendingRetirement {
                action,
                running: false,
            });
        Ok(())
    }
    /// Fence observations before publishing a newly registered native writer.
    pub(super) fn admitting(&self, id: &str) -> Result<(), ErrorCode> {
        let mut observations = self
            .shared
            .observations
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?;
        let sender = observations.entry(id.to_owned()).or_insert_with(|| {
            watch::channel(
                json!({"status":"running","final":null,"error":null,"lastMessage":null,
                "live":true,"busy":true}),
            )
            .0
        });
        sender.send_if_modified(|value| {
            if value["busy"] == true && value["live"] == true {
                return false;
            }
            value["busy"] = json!(true);
            value["live"] = json!(true);
            true
        });
        Ok(())
    }
    /// Claim placement before launch, rejecting a newly discovered retiring Workspace.
    pub(super) fn place(&self, workspace: &str) -> Result<(), ErrorCode> {
        let scope = format!("workspace:{workspace}");
        let added = self
            .shared
            .routes
            .lock()
            .map_err(|_| ErrorCode::AgentIo)?
            .scopes
            .entry(self.lane.clone())
            .or_default()
            .insert(scope.clone());
        // A placement discovered after admission must not launch a writer into a retiring scope.
        if added
            && self
                .shared
                .barriers
                .lock()
                .map_err(|_| ErrorCode::AgentIo)?
                .get(&scope)
                .is_some_and(|tokens| tokens.iter().any(|token| !token.is_cancelled()))
        {
            if let Some(scopes) = self
                .shared
                .routes
                .lock()
                .map_err(|_| ErrorCode::AgentIo)?
                .scopes
                .get_mut(&self.lane)
            {
                scopes.remove(&scope);
            }
            return Err(ErrorCode::CatalogBusy);
        }
        Ok(())
    }

    /// Release inactive routing aliases after native cleanup.
    pub(super) fn release(&self) {
        self.shared.release(&self.lane);
    }
    /// Bind both host and native identities to this single writer owner.
    pub(super) fn bind(&self, record: &PersistedAgentRuntimeRecord) -> Result<(), ErrorCode> {
        self.shared.bind(&self.lane, record)
    }

    /// Whether an identity's mutable session state belongs to this lane.
    pub(super) fn owns(&self, id: &str) -> bool {
        self.shared
            .routes
            .lock()
            .is_ok_and(|routes| routes.agents.get(id) == Some(&self.lane))
    }

    /// Return this lane's identities for input filtering and committed publication.
    pub(crate) fn agents(&self) -> Vec<String> {
        self.shared
            .routes
            .lock()
            .map(|routes| {
                routes
                    .agents
                    .iter()
                    .filter(|(_, lane)| **lane == self.lane)
                    .map(|(id, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
