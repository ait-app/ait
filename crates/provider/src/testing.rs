//! Runtime entry points for adapter crates' tests, compiled only with the `test-support` feature.
//!
//! Production dependents never enable the feature, and the dependency guard rejects non-dev
//! edges that do. Each item delegates to a crate-private runtime operation, so testing an
//! adapter from its own crate does not widen the runtime's production API.

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use model::ErrorCode;
use model::events::EventHub;
use serde_json::Value;

use crate::ports::agent_session::{AgentClient, AgentSessionSpec};
use crate::ports::controls::NativeSubagent;
use crate::protocol::timeline::NativeItem;
use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
use crate::storage::timeline::Timeline;

#[cfg(test)]
mod tests;

/// Foreground manager driven directly by a test, without the execution worker.
#[derive(Debug)]
pub struct Manager(AgentManager);

impl Manager {
    /// Drive `manager`; build it with `AgentManager::with_timeline` to persist display history.
    #[must_use]
    pub const fn new(manager: AgentManager) -> Self {
        Self(manager)
    }

    /// Register one native adapter under its provider identity.
    ///
    /// # Errors
    /// Returns invalid or duplicate provider identities.
    pub fn register(&mut self, client: Box<dyn AgentClient>) -> Result<(), AgentManagerError> {
        self.0.register_client(client)
    }

    /// Create `agent_id` from `spec` with a default, user-visible registration.
    ///
    /// # Errors
    /// Returns invalid or duplicate IDs, unavailable providers, and session or storage failures.
    pub async fn create(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0
            .create(agent_id, spec, AgentRegistration::default())
            .await
    }

    /// Reopen the persisted native session of `agent_id`.
    ///
    /// # Errors
    /// Returns unknown agents, unavailable providers, and native resume failures.
    pub async fn resume(
        &mut self,
        agent_id: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.0.resume(agent_id).await
    }

    /// Submit one text prompt to `agent_id`.
    ///
    /// # Errors
    /// Returns resume, provider, storage, or busy/read-only failures.
    pub async fn send(&mut self, agent_id: &str, text: &str) -> Result<(), AgentManagerError> {
        self.0.send(agent_id, text).await
    }

    /// Cancel the active turn of `agent_id`.
    ///
    /// # Errors
    /// Returns unknown agents and native cancellation or storage failures.
    pub async fn cancel(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        self.0.cancel(agent_id).await
    }

    /// Drain pending native events of every live session once.
    ///
    /// # Errors
    /// Returns storage failures while recording events.
    pub async fn poll(&mut self) -> Result<(), AgentManagerError> {
        self.0.poll().await
    }

    /// Close every live session, reporting the first failure after attempting all of them.
    ///
    /// # Errors
    /// Returns the first native close or storage failure.
    pub async fn close_all(&mut self) -> Result<(), AgentManagerError> {
        self.0.close_all().await
    }

    /// The active turn of `agent_id`, if one is running.
    #[must_use]
    pub fn active_turn(&self, agent_id: &str) -> Option<&str> {
        self.0.active_turn(agent_id)
    }

    /// The live record of a registered `agent_id`.
    #[must_use]
    pub fn live_snapshot(&self, agent_id: &str) -> Option<&PersistedAgentRuntimeRecord> {
        self.0.live_snapshot(agent_id)
    }

    /// Fill the published `snapshot` of `record` with its capabilities, modes and live controls.
    pub fn control_snapshot(&self, record: &PersistedAgentRuntimeRecord, snapshot: &mut Value) {
        self.0.control_snapshot(record, snapshot);
    }
}

/// Open an empty in-memory display timeline.
///
/// # Errors
/// Returns schema initialization failures.
pub fn memory_timeline() -> Result<Timeline, ErrorCode> {
    Timeline::memory()
}

/// The change notifications published by `timeline`.
#[must_use]
pub fn timeline_events(timeline: &Timeline) -> EventHub {
    timeline.events()
}

/// The generation and ordered display entries of `agent`.
///
/// # Errors
/// Returns storage or invalid persisted-data errors.
pub fn timeline_entries(
    timeline: &Timeline,
    agent: &str,
) -> Result<(String, Vec<NativeItem>), ErrorCode> {
    let (epoch, rows) = timeline.read(agent)?;
    Ok((epoch, rows.into_iter().map(|row| row.entry).collect()))
}

/// Append `entries` produced by `provider` to the display history of `agent`.
///
/// # Errors
/// Returns storage or invalid-entry errors.
pub fn append_timeline(
    timeline: &Timeline,
    agent: &str,
    provider: &str,
    entries: &[NativeItem],
) -> Result<(String, Vec<u64>), ErrorCode> {
    timeline.append(agent, provider, entries)
}

/// Replace the display history of `agent` with `entries`, returning the new generation.
///
/// # Errors
/// Returns storage or invalid-entry errors.
pub fn reconcile_timeline(
    timeline: &Timeline,
    agent: &str,
    provider: &str,
    entries: &[NativeItem],
) -> Result<String, ErrorCode> {
    timeline.reconcile(agent, provider, entries)
}

/// The stored native subagent descriptors of `parent`.
///
/// # Errors
/// Returns storage failures or an undecodable stored descriptor.
pub fn timeline_subagents(
    timeline: &Timeline,
    parent: &str,
) -> Result<Vec<NativeSubagent>, ErrorCode> {
    timeline.subagents(parent)
}
