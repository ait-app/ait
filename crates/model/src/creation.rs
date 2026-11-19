//! Durable creation progress shared by Workspace and Agent capabilities.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use domain::creation::protocol::{Kind, Phase, Snapshot};
use domain::creation::{Admission, Receipt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ErrorCode;
use crate::events::{EventHub, Subscription};
use crate::outbound::Outbound;

/// Stable request fingerprints shared with legacy receipt migration.
pub mod fingerprint;

/// Blocking persistence boundary for creation receipts; implementations own storage mechanics.
pub trait ReceiptStore: std::fmt::Debug + Send + Sync {
    /// Return the receipt for `id`, or none when it has not been committed.
    /// # Errors
    /// Returns `RegistryIo` when storage is unavailable.
    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode>;

    /// Atomically store a receipt and its resource claims before publishing progress.
    /// `expected_revision` is absent for insertion, or the revision being replaced for an update.
    /// # Errors
    /// Returns `IdempotencyConflict` for stale writes or conflicting keys/resources, and `RegistryIo`
    /// for storage failures or `ResourceExhausted` for a full database.
    /// Failures preserve the prior receipt and resource claims.
    fn put(&self, receipt: Receipt, expected_revision: Option<u64>) -> Result<(), ErrorCode>;
}

#[derive(Debug, Default)]
struct State {
    memory: BTreeMap<String, Receipt>,
    active: BTreeSet<String>,
    claimed_workspaces: BTreeSet<String>,
    claimed_agents: BTreeSet<String>,
}

/// Shared creation coordinator with injected persistence; embedded defaults are ephemeral.
#[derive(Debug, Clone, Default)]
pub struct Creations {
    store: Option<Arc<dyn ReceiptStore>>,
    state: Arc<Mutex<State>>,
    events: EventHub,
}

impl Creations {
    /// Observe committed creation receipts independently of client subscriptions.
    pub fn set_publisher(&self, publisher: Arc<dyn crate::hub::EventPublisher>) {
        self.events.set_publisher(publisher);
    }

    /// Attach an initialized store and query durable claims on demand.
    #[must_use]
    pub fn with_store(store: Arc<dyn ReceiptStore>) -> Self {
        Self {
            store: Some(store),
            ..Self::default()
        }
    }

    /// Atomically reserve a resource identity for an immutable creation intent.
    /// `intent` excludes the key and subscribe flag; retries must supply the same intent.
    /// # Errors
    /// Returns invalid keys, key conflicts or durable storage failures before any side effects.
    pub fn begin(&self, kind: Kind, key: &str, intent: Value) -> Result<Admission, ErrorCode> {
        validate_key(key)?;
        let intent_hash = fingerprint::of(&intent);
        let workspace_id = intent["workspaceId"].as_str().map(str::to_owned);
        let (needs_agent, agent_id) = match kind {
            Kind::Agent => (true, intent["agentId"].as_str()),
            Kind::Workspace => (
                intent["agent"].is_object(),
                intent["agent"]["agentId"].as_str(),
            ),
        };
        let agent_id = agent_id.map(str::to_owned);
        // Release request images and attachments before waiting for receipt I/O.
        drop(intent);
        let id = identity(kind, key);
        let mut state = self.state.lock().map_err(io)?;
        if let Some(receipt) = self.read(&state, &id)? {
            return self.replay(&mut state, receipt, &intent_hash);
        }
        let workspace_id = match (kind, workspace_id) {
            (Kind::Workspace, None) => {
                Some(domain::workspace::registry::generate_workspace_id().map_err(io)?)
            }
            (_, workspace_id) => workspace_id,
        };
        let snapshot = Snapshot {
            kind,
            idempotency_key: key.to_owned(),
            revision: 0,
            phase: Phase::Accepted,
            workspace_id,
            agent_id: agent_id.or_else(|| needs_agent.then(|| Uuid::new_v4().to_string())),
            error: None,
            outcome_unknown: false,
            workspace: None,
            agent: None,
        };
        self.write(
            &mut state,
            Receipt {
                id: id.clone(),
                intent_hash,
                snapshot: snapshot.clone(),
                retry_initial_agent: false,
            },
            None,
        )?;
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
        intent_hash: &[u8; 32],
    ) -> Result<Admission, ErrorCode> {
        if &receipt.intent_hash != intent_hash {
            return Err(ErrorCode::IdempotencyConflict);
        }
        let execute = receipt.retry_initial_agent && !state.active.contains(&receipt.id);
        if execute {
            let revision = receipt.snapshot.revision;
            receipt.retry_initial_agent = false;
            receipt.snapshot.revision = receipt
                .snapshot
                .revision
                .checked_add(1)
                .ok_or(ErrorCode::ResourceExhausted)?;
            receipt.snapshot.phase = Phase::WorkspaceReady;
            receipt.snapshot.error = None;
            let id = receipt.id.clone();
            self.write(state, receipt.clone(), Some(revision))?;
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
            || receipt.snapshot.phase != Phase::Failed
            || receipt.snapshot.agent.is_some()
            || receipt.snapshot.agent_id.is_none()
            || receipt.snapshot.workspace.is_none()
            || receipt.snapshot.outcome_unknown
        {
            return Err(ErrorCode::InvalidMessage);
        }
        receipt.retry_initial_agent = true;
        self.write(&mut state, receipt, Some(snapshot.revision))
    }

    /// Commit a progress transition and publish it after durable installation.
    /// # Errors
    /// Returns absent receipts, invalid transitions or storage failures. Terminal receipts cannot change.
    pub fn advance(
        &self,
        snapshot: &Snapshot,
        phase: Phase,
        result: Option<Value>,
        error: Option<String>,
    ) -> Result<Snapshot, ErrorCode> {
        if phase == Phase::Accepted {
            return Err(ErrorCode::InvalidMessage);
        }
        let id = identity(snapshot.kind, &snapshot.idempotency_key);
        let mut state = self.state.lock().map_err(io)?;
        let mut receipt = self.read(&state, &id)?.ok_or(ErrorCode::InvalidMessage)?;
        if receipt.snapshot.phase.is_terminal() || receipt.snapshot.revision != snapshot.revision {
            return Err(ErrorCode::IdempotencyConflict);
        }
        receipt.snapshot.revision = receipt
            .snapshot
            .revision
            .checked_add(1)
            .ok_or(ErrorCode::ResourceExhausted)?;
        receipt.snapshot.phase = phase;
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
        receipt.snapshot.outcome_unknown =
            phase == Phase::Failed && receipt.snapshot.agent.is_some();
        let expected_revision = snapshot.revision;
        let snapshot = receipt.snapshot.clone();
        self.write(&mut state, receipt, Some(expected_revision))?;
        if phase.is_terminal() {
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

    fn write(
        &self,
        state: &mut State,
        receipt: Receipt,
        expected_revision: Option<u64>,
    ) -> Result<(), ErrorCode> {
        if let Some(store) = &self.store {
            store.put(receipt, expected_revision)?;
        } else {
            if expected_revision.is_none() {
                let (workspace, agent) = receipt.snapshot.resource_claims();
                if workspace.is_some_and(|id| state.claimed_workspaces.contains(id))
                    || agent.is_some_and(|id| state.claimed_agents.contains(id))
                {
                    return Err(ErrorCode::IdempotencyConflict);
                }
                state
                    .claimed_workspaces
                    .extend(workspace.map(str::to_owned));
                state.claimed_agents.extend(agent.map(str::to_owned));
            }
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
fn observed(mut snapshot: Snapshot, active: bool) -> Snapshot {
    if !snapshot.phase.is_terminal() && !active {
        snapshot.outcome_unknown = true;
        snapshot.phase = Phase::Failed;
        snapshot.error = Some("Creation was interrupted; inspect the reserved resource before retrying with a new key".to_owned());
    }
    snapshot
}
fn io<T>(_: T) -> ErrorCode {
    ErrorCode::RegistryIo
}

#[cfg(test)]
mod tests;
