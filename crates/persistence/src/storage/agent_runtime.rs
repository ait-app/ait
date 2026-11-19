//! Durable Agent snapshots cached in memory by [`status::AgentsStatus`].

use std::path::PathBuf;
use std::sync::Arc;

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::{AgentRuntimeRegistry, AgentRuntimeRegistryError};

use status::{AgentsStatus, Durability};

mod status;

/// Agent runtime snapshots cached in memory and persisted to one JSON document.
/// Status updates are written within one flush interval; other mutations are written at once.
#[derive(Debug, Clone)]
pub struct FileBackedAgentRuntimeRegistry {
    status: Arc<AgentsStatus>,
    changes: Option<model::changes::Changes>,
}

impl FileBackedAgentRuntimeRegistry {
    /// Create a lazy registry backed by the JSON document at `path`.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            status: Arc::new(AgentsStatus::new(path)),
            changes: None,
        }
    }

    /// Signal subscribed Workspace projections after Agent mutations.
    #[must_use]
    pub fn with_changes(mut self, changes: model::changes::Changes) -> Self {
        self.changes = Some(changes);
        self
    }

    /// Write deferred status updates now. The last owner also writes them when it drops.
    ///
    /// # Errors
    /// Returns `Io` when the write fails; the updates stay pending for the next flush.
    pub fn flush(&self) -> Result<(), AgentRuntimeRegistryError> {
        self.status.flush()
    }

    /// Write the final document marked as cleanly closed, then reject later mutations from
    /// every clone. Hosts call this before releasing the data directory, so a lingering owner
    /// cannot overwrite the file a later runtime owns.
    ///
    /// # Errors
    /// Returns `Io` when the final write fails; the registry is closed regardless.
    pub fn close(&self) -> Result<(), AgentRuntimeRegistryError> {
        self.status.close()
    }

    /// Return the loaded file's last change time when its writer did not close it, as a UTC
    /// RFC 3339 timestamp. Deferred status updates made after that time may be missing. A
    /// cleanly closed, legacy or missing file reports `None`.
    ///
    /// # Errors
    /// Returns load failures.
    pub fn interrupted_since(&self) -> Result<Option<String>, AgentRuntimeRegistryError> {
        self.status.interrupted_since()
    }

    fn change(
        &self,
        agent_id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
        durability: Durability,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        let Some((record, changed)) = self.status.update(
            agent_id,
            |current| {
                let next = update(current);
                validate_record(&next)?;
                if next.id == agent_id {
                    Ok(next)
                } else {
                    Err(AgentRuntimeRegistryError::InvalidRecord)
                }
            },
            durability,
        )?
        else {
            return Ok(None);
        };
        if changed {
            self.notify();
        }
        Ok(Some(record))
    }

    fn notify(&self) {
        if let Some(changes) = &self.changes {
            changes.notify();
        }
    }
}

impl AgentRuntimeRegistry for FileBackedAgentRuntimeRegistry {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        self.status.initialize()
    }

    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self
            .status
            .snapshot()?
            .iter()
            .map(|record| record.as_ref().clone())
            .collect())
    }

    fn snapshot(
        &self,
    ) -> Result<Arc<[Arc<PersistedAgentRuntimeRecord>]>, AgentRuntimeRegistryError> {
        self.status.snapshot()
    }

    fn get(
        &self,
        agent_id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        Ok(self
            .status
            .get_shared(agent_id)?
            .map(|record| record.as_ref().clone()))
    }

    fn get_shared(
        &self,
        agent_id: &str,
    ) -> Result<Option<Arc<PersistedAgentRuntimeRecord>>, AgentRuntimeRegistryError> {
        self.status.get_shared(agent_id)
    }

    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        validate_record(record)?;
        if self.status.upsert(record)? {
            self.notify();
        }
        Ok(())
    }

    fn update(
        &self,
        agent_id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.change(agent_id, update, Durability::Immediate)
    }

    fn update_status(
        &self,
        agent_id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.change(agent_id, update, Durability::Deferred)
    }

    fn remove(&self, agent_id: &str) -> Result<bool, AgentRuntimeRegistryError> {
        let removed = self.status.remove(agent_id)?;
        if removed {
            self.notify();
        }
        Ok(removed)
    }
}

fn validate_record(record: &PersistedAgentRuntimeRecord) -> Result<(), AgentRuntimeRegistryError> {
    if let Some(source) = &record.observation
        && (record.persistence.is_some()
            || source.root_agent_id == record.id
            || source.parent_agent_id == record.id
            || source
                .persistence
                .as_ref()
                .is_some_and(|handle| handle.provider != record.provider)
            || record.labels.get("paseo.parent-agent-id") != Some(&source.parent_agent_id)
            || [
                &source.root_agent_id,
                &source.root_session_id,
                &source.parent_agent_id,
                &source.native_id,
                &source.parent_native_id,
                &source.observed_at,
            ]
            .iter()
            .any(|value| {
                value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
            }))
    {
        return Err(AgentRuntimeRegistryError::InvalidRecord);
    }
    let required = [
        record.id.as_str(),
        record.provider.as_str(),
        record.cwd.as_str(),
        record.created_at.as_str(),
        record.updated_at.as_str(),
    ];
    if required
        .iter()
        .any(|value| value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control))
        || record.id.len() > 128
    {
        return Err(AgentRuntimeRegistryError::InvalidRecord);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
