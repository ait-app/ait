//! Durable Agent snapshots backed by the shared atomic file engine.

use std::path::PathBuf;
use std::sync::Arc;

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::{AgentRuntimeRegistry, AgentRuntimeRegistryError};

use crate::registry::{Error as RegistryError, FileRegistry};

/// Atomically persisted registry of Paseo Agent runtime snapshots.
#[derive(Debug, Clone)]
pub struct FileBackedAgentRuntimeRegistry {
    file: Arc<FileRegistry<PersistedAgentRuntimeRecord>>,
    changes: Option<model::changes::Changes>,
}

impl FileBackedAgentRuntimeRegistry {
    /// Create a lazy registry backed by one JSON array.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            file: Arc::new(FileRegistry::new(path, |record| &record.id)),
            changes: None,
        }
    }

    /// Signal subscribed Workspace projections after durable Agent mutations.
    #[must_use]
    pub fn with_changes(mut self, changes: model::changes::Changes) -> Self {
        self.changes = Some(changes);
        self
    }
}

impl AgentRuntimeRegistry for FileBackedAgentRuntimeRegistry {
    fn initialize(&self) -> Result<(), AgentRuntimeRegistryError> {
        self.file.initialize().map_err(map_error)
    }

    fn list(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.file.list().map_err(map_error)
    }

    fn snapshot(
        &self,
    ) -> Result<Arc<[Arc<PersistedAgentRuntimeRecord>]>, AgentRuntimeRegistryError> {
        self.file.snapshot().map_err(map_error)
    }

    fn get(
        &self,
        agent_id: &str,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        self.file.get(agent_id).map_err(map_error)
    }

    fn get_shared(
        &self,
        agent_id: &str,
    ) -> Result<Option<Arc<PersistedAgentRuntimeRecord>>, AgentRuntimeRegistryError> {
        self.file.get_shared(agent_id).map_err(map_error)
    }

    fn upsert(
        &self,
        record: &PersistedAgentRuntimeRecord,
    ) -> Result<(), AgentRuntimeRegistryError> {
        validate_record(record)?;
        self.file
            .mutate(|records| {
                records.insert(record.id.clone(), record.clone());
                Ok(((), true))
            })
            .map_err(map_error)?;
        if let Some(changes) = &self.changes {
            changes.notify();
        }
        Ok(())
    }

    fn update(
        &self,
        agent_id: &str,
        update: &dyn Fn(&PersistedAgentRuntimeRecord) -> PersistedAgentRuntimeRecord,
    ) -> Result<Option<PersistedAgentRuntimeRecord>, AgentRuntimeRegistryError> {
        let result = self
            .file
            .mutate(|records| {
                let Some(current) = records.get(agent_id) else {
                    return Ok((None, false));
                };
                let next = update(current);
                validate_record(&next).map_err(unmap_error)?;
                if next.id != agent_id {
                    return Err(RegistryError::InvalidRecord);
                }
                records.insert(agent_id.to_owned(), next.clone());
                Ok((Some(next), true))
            })
            .map_err(map_error)?;
        if result.is_some()
            && let Some(changes) = &self.changes
        {
            changes.notify();
        }
        Ok(result)
    }

    fn remove(&self, agent_id: &str) -> Result<bool, AgentRuntimeRegistryError> {
        let removed = self
            .file
            .mutate(|records| {
                let removed = records.shift_remove(agent_id).is_some();
                Ok((removed, removed))
            })
            .map_err(map_error)?;
        if removed && let Some(changes) = &self.changes {
            changes.notify();
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

const fn map_error(error: RegistryError) -> AgentRuntimeRegistryError {
    match error {
        RegistryError::InvalidRecord => AgentRuntimeRegistryError::InvalidRecord,
        RegistryError::InvalidFile => AgentRuntimeRegistryError::InvalidFile,
        RegistryError::Io | RegistryError::Frozen => AgentRuntimeRegistryError::Io,
    }
}

const fn unmap_error(error: AgentRuntimeRegistryError) -> RegistryError {
    match error {
        AgentRuntimeRegistryError::InvalidRecord => RegistryError::InvalidRecord,
        AgentRuntimeRegistryError::InvalidFile => RegistryError::InvalidFile,
        AgentRuntimeRegistryError::Io => RegistryError::Io,
    }
}

#[cfg(test)]
mod tests;
