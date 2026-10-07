//! Durable creation progress shared by Workspace and Agent capabilities.

pub mod protocol;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ErrorCode;
use crate::creation::protocol::{Kind, Snapshot};
use crate::events::{EventHub, Subscription};
use crate::outbound::Outbound;

/// Persisted immutable creation intent and its latest committed progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Opaque, kind-qualified digest of the idempotency key.
    pub id: String,
    /// Original request excluding its key and subscription flag.
    pub intent: Value,
    /// Latest committed progress and reserved identities.
    pub snapshot: Snapshot,
    /// A proven initial Agent startup failure may retry the reserved identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retry_initial_agent: bool,
}

/// Blocking persistence boundary for creation receipts; implementations own storage mechanics.
pub trait ReceiptStore: std::fmt::Debug + Send + Sync {
    /// Return all committed receipts to restore resource claims at startup.
    /// # Errors
    /// Returns `RegistryIo` if persisted state cannot be loaded.
    fn list(&self) -> Result<Vec<Receipt>, ErrorCode>;

    /// Return the receipt for `id`, or none when it has not been committed.
    /// # Errors
    /// Returns `RegistryIo` when storage is unavailable.
    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode>;

    /// Durably replace `receipt` before publishing its progress or claiming resources.
    /// # Errors
    /// Returns `RegistryIo` on failure; implementations preserve the prior committed record.
    fn put(&self, receipt: Receipt) -> Result<(), ErrorCode>;
}

#[derive(Debug, Default)]
struct State {
    memory: BTreeMap<String, Receipt>,
    active: BTreeSet<String>,
    claimed: BTreeSet<String>,
}

/// Shared creation coordinator with injected persistence; embedded defaults are ephemeral.
#[derive(Debug, Clone, Default)]
pub struct Creations {
    store: Option<Arc<dyn ReceiptStore>>,
    state: Arc<Mutex<State>>,
    events: EventHub,
}

/// Admission result: only a newly accepted intent may perform resource side effects.
#[derive(Debug)]
pub struct Admission {
    /// Whether the caller owns the first attempt.
    pub execute: bool,
    /// Latest committed state, including IDs reserved before the side effect.
    pub snapshot: Snapshot,
}

impl Creations {
    /// Load receipts from `store` without retrying interrupted work; return a shared coordinator.
    /// # Errors
    /// Returns the store's load error before accepting any new creation side effects.
    pub fn with_store(store: Arc<dyn ReceiptStore>) -> Result<Self, ErrorCode> {
        let claimed = store
            .list()?
            .iter()
            .flat_map(|receipt| owned_resources(&receipt.snapshot))
            .collect();
        Ok(Self {
            store: Some(store),
            state: Arc::new(Mutex::new(State {
                claimed,
                ..State::default()
            })),
            ..Self::default()
        })
    }

    /// Atomically reserve a resource identity for an immutable creation intent.
    /// `intent` excludes the key and subscribe flag; retries must supply the same intent.
    /// # Errors
    /// Returns invalid keys, key conflicts or durable storage failures before any side effects.
    pub fn begin(&self, kind: Kind, key: &str, intent: Value) -> Result<Admission, ErrorCode> {
        validate_key(key)?;
        let id = identity(kind, key);
        let mut state = self.state.lock().map_err(io)?;
        if let Some(receipt) = self.read(&state, &id)? {
            return self.replay(&mut state, receipt, &intent);
        }
        let field = match kind {
            Kind::Agent => "agentId",
            Kind::Workspace => "workspaceId",
        };
        let resource_id = if let Some(id) = intent[field].as_str() {
            id.to_owned()
        } else {
            match kind {
                Kind::Agent => Uuid::new_v4().to_string(),
                Kind::Workspace => {
                    crate::workspace::registry::generate_workspace_id().map_err(io)?
                }
            }
        };
        let snapshot = Snapshot {
            kind,
            idempotency_key: key.to_owned(),
            revision: 0,
            phase: "accepted".to_owned(),
            workspace_id: if kind == Kind::Workspace {
                Some(resource_id.clone())
            } else {
                intent["workspaceId"].as_str().map(str::to_owned)
            },
            agent_id: if kind == Kind::Agent {
                Some(resource_id)
            } else if intent["agent"].is_object() {
                Some(
                    intent["agent"]["agentId"]
                        .as_str()
                        .map_or_else(|| Uuid::new_v4().to_string(), str::to_owned),
                )
            } else {
                None
            },
            error: None,
            outcome_unknown: false,
            workspace: None,
            agent: None,
        };
        let resources: Vec<_> = owned_resources(&snapshot).collect();
        if resources
            .iter()
            .any(|resource| state.claimed.contains(resource))
        {
            return Err(ErrorCode::IdempotencyConflict);
        }
        self.write(
            &mut state,
            Receipt {
                id: id.clone(),
                intent,
                snapshot: snapshot.clone(),
                retry_initial_agent: false,
            },
        )?;
        state.claimed.extend(resources);
        state.active.insert(id.clone());
        self.publish(&id, &snapshot);
        Ok(Admission {
            execute: true,
            snapshot,
        })
    }

    fn replay(
        &self,
        state: &mut State,
        mut receipt: Receipt,
        intent: &Value,
    ) -> Result<Admission, ErrorCode> {
        if &receipt.intent != intent {
            return Err(ErrorCode::IdempotencyConflict);
        }
        let execute = receipt.retry_initial_agent && !state.active.contains(&receipt.id);
        if execute {
            receipt.retry_initial_agent = false;
            receipt.snapshot.revision = receipt
                .snapshot
                .revision
                .checked_add(1)
                .ok_or(ErrorCode::ResourceExhausted)?;
            "workspace_ready".clone_into(&mut receipt.snapshot.phase);
            receipt.snapshot.error = None;
            let id = receipt.id.clone();
            self.write(state, receipt.clone())?;
            state.active.insert(id.clone());
            self.publish(&id, &receipt.snapshot);
        }
        Ok(Admission {
            execute,
            snapshot: observed(receipt.snapshot, state.active.contains(&receipt.id)),
        })
    }

    /// Allow retrying only the initial Agent of an already provisioned Workspace.
    /// The caller must prove that no Agent was registered and all failed native children closed.
    /// Prompt attempts and uncertain resource outcomes must never use this operation.
    /// # Errors
    /// Rejects stale/nonfailed receipts, absent Workspace/Agent identities, or persistence failures.
    pub fn allow_initial_agent_retry(&self, snapshot: &Snapshot) -> Result<(), ErrorCode> {
        let id = identity(snapshot.kind, &snapshot.idempotency_key);
        let mut state = self.state.lock().map_err(io)?;
        let mut receipt = self.read(&state, &id)?.ok_or(ErrorCode::InvalidMessage)?;
        if snapshot.kind != Kind::Workspace
            || receipt.snapshot.revision != snapshot.revision
            || receipt.snapshot.phase != "failed"
            || receipt.snapshot.agent.is_some()
            || receipt.snapshot.agent_id.is_none()
            || receipt.snapshot.workspace.is_none()
            || receipt.snapshot.outcome_unknown
        {
            return Err(ErrorCode::InvalidMessage);
        }
        receipt.retry_initial_agent = true;
        self.write(&mut state, receipt)
    }

    /// Commit a progress transition and publish it after durable installation.
    /// # Errors
    /// Returns absent receipts, invalid transitions or storage failures. Terminal receipts cannot change.
    pub fn advance(
        &self,
        snapshot: &Snapshot,
        phase: &str,
        result: Option<Value>,
        error: Option<String>,
    ) -> Result<Snapshot, ErrorCode> {
        if !matches!(
            phase,
            "workspace_ready" | "agent_ready" | "prompt_started" | "completed" | "failed"
        ) {
            return Err(ErrorCode::InvalidMessage);
        }
        let id = identity(snapshot.kind, &snapshot.idempotency_key);
        let mut state = self.state.lock().map_err(io)?;
        let mut receipt = self.read(&state, &id)?.ok_or(ErrorCode::InvalidMessage)?;
        if terminal(&receipt.snapshot.phase) || receipt.snapshot.revision != snapshot.revision {
            return Err(ErrorCode::IdempotencyConflict);
        }
        receipt.snapshot.revision = receipt
            .snapshot
            .revision
            .checked_add(1)
            .ok_or(ErrorCode::ResourceExhausted)?;
        phase.clone_into(&mut receipt.snapshot.phase);
        receipt.snapshot.error = error;
        if let Some(result) = result {
            if let Some(agent) = result.get("agent").filter(|value| !value.is_null()) {
                if let Some(workspace) = agent["workspaceId"].as_str() {
                    receipt.snapshot.workspace_id = Some(workspace.to_owned());
                }
                receipt.snapshot.agent = Some(agent.clone());
            }
            if let Some(workspace) = result.get("workspace").filter(|value| !value.is_null()) {
                receipt.snapshot.workspace = Some(workspace.clone());
            }
        }
        // Once an Agent is ready, a failure may follow acceptance of the initial prompt.
        receipt.snapshot.outcome_unknown = phase == "failed" && receipt.snapshot.agent.is_some();
        let snapshot = receipt.snapshot.clone();
        self.write(&mut state, receipt)?;
        if terminal(phase) {
            state.active.remove(&id);
        }
        self.publish(&id, &snapshot);
        Ok(snapshot)
    }

    /// Read the latest receipt, reporting interrupted side effects as uncertain after restart.
    /// # Errors
    /// Returns invalid keys or registry failures.
    pub fn snapshot(&self, kind: Kind, key: &str) -> Result<Option<Snapshot>, ErrorCode> {
        validate_key(key)?;
        let state = self.state.lock().map_err(io)?;
        let id = identity(kind, key);
        Ok(self
            .read(&state, &id)?
            .map(|receipt| observed(receipt.snapshot, state.active.contains(&id))))
    }

    /// Atomically capture a snapshot and install a paused observer, including for an unknown key.
    /// # Errors
    /// Returns invalid keys or storage failures. Activate only after sending the response.
    pub fn subscribe(
        &self,
        kind: Kind,
        key: &str,
        outbound: Outbound,
    ) -> Result<(Option<Snapshot>, Subscription), ErrorCode> {
        validate_key(key)?;
        let state = self.state.lock().map_err(io)?;
        let id = identity(kind, key);
        let snapshot = self
            .read(&state, &id)?
            .map(|receipt| observed(receipt.snapshot, state.active.contains(&id)));
        let subscription =
            self.events
                .subscribe(Uuid::new_v4().to_string(), BTreeSet::from([id]), outbound);
        Ok((snapshot, subscription))
    }

    /// Observe progress for one create operation without a connection subscription ID on events.
    /// The caller activates before executing creation and drops the guard when it completes.
    /// # Errors
    /// Returns invalid keys or a poisoned coordinator lock.
    pub fn observe(
        &self,
        kind: Kind,
        key: &str,
        outbound: Outbound,
    ) -> Result<Subscription, ErrorCode> {
        validate_key(key)?;
        let _state = self.state.lock().map_err(io)?;
        Ok(self.events.observe(
            Uuid::new_v4().to_string(),
            BTreeSet::from([identity(kind, key)]),
            outbound,
        ))
    }

    fn read(&self, state: &State, id: &str) -> Result<Option<Receipt>, ErrorCode> {
        match &self.store {
            Some(store) => store.get(id),
            None => Ok(state.memory.get(id).cloned()),
        }
    }

    fn write(&self, state: &mut State, receipt: Receipt) -> Result<(), ErrorCode> {
        if let Some(store) = &self.store {
            store.put(receipt)?;
        } else {
            state.memory.insert(receipt.id.clone(), receipt);
        }
        Ok(())
    }

    fn publish(&self, id: &str, snapshot: &Snapshot) {
        let method = match snapshot.kind {
            Kind::Agent => "agent.create.update",
            Kind::Workspace => "workspace.create.update",
        };
        self.events.publish(id, method, &json!(snapshot));
    }
}

fn owned_resources(snapshot: &Snapshot) -> impl Iterator<Item = String> {
    [
        snapshot
            .workspace_id
            .as_ref()
            .filter(|_| snapshot.kind == Kind::Workspace)
            .map(|id| format!("workspace:{id}")),
        snapshot.agent_id.as_ref().map(|id| format!("agent:{id}")),
    ]
    .into_iter()
    .flatten()
}

/// Validate a bounded durable key without assigning it filesystem path semantics.
/// # Errors
/// Rejects empty, oversized or control-containing keys.
pub fn validate_key(key: &str) -> Result<(), ErrorCode> {
    if key.is_empty() || key.len() > 512 || key.chars().any(char::is_control) {
        Err(ErrorCode::InvalidMessage)
    } else {
        Ok(())
    }
}

fn identity(kind: Kind, key: &str) -> String {
    format!("{}:{:x}", kind.name(), Sha256::digest(key.as_bytes()))
}
fn terminal(phase: &str) -> bool {
    matches!(phase, "completed" | "failed")
}
fn observed(mut snapshot: Snapshot, active: bool) -> Snapshot {
    if !terminal(&snapshot.phase) && !active {
        snapshot.outcome_unknown = true;
        "failed".clone_into(&mut snapshot.phase);
        snapshot.error = Some("Creation was interrupted; inspect the reserved resource before retrying with a new key".to_owned());
    }
    snapshot
}
fn io<T>(_: T) -> ErrorCode {
    ErrorCode::RegistryIo
}

#[cfg(test)]
mod tests;
