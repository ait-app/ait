use domain::agent_runtime::PersistedAgentRuntimeRecord;

use super::{
    AgentManager, AgentManagerError, Registration, map_registry, map_session, validate_identity,
};
use crate::ports::agent_session::{AgentResumePurpose, AgentSessionSpec};
use crate::protocol::agent_config::NullableSetting;
use crate::protocol::resume::Overrides;

impl AgentManager {
    /// Register unknown native history only after interactive restore succeeds.
    /// # Errors
    /// Rejects duplicate identity, exhausted capacity, invalid handles, or native/registry failures.
    pub(crate) async fn restore_new(
        &mut self,
        record: PersistedAgentRuntimeRecord,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        let id = record.id.clone();
        if self.live.len() >= 32 {
            return Err(AgentManagerError::Busy);
        }
        if self.live.contains_key(&id) || self.registry.get(&id).map_err(map_registry)?.is_some() {
            return Err(AgentManagerError::AlreadyExists(id));
        }
        validate_overrides(&record, &Overrides::default())?;
        if record.title.as_ref().is_some_and(|title| {
            title.trim().is_empty() || title.trim().encode_utf16().count() > 200
        }) {
            return Err(AgentManagerError::InvalidRequest);
        }
        let spec = AgentSessionSpec {
            provider: record.provider.clone(),
            cwd: record.cwd.clone(),
            config: record.config.clone().unwrap_or_default(),
        };
        validate_identity(&id, &spec)?;
        if let Some(workspace) = &record.workspace_id {
            self.place_workspace(workspace)
                .map_err(|_| AgentManagerError::Busy)?;
        }
        let permit = self.reserve_session()?;
        let client = self.available_client(&spec.provider).await?;
        client
            .validate_selection(&spec)
            .await
            .map_err(|_| AgentManagerError::InvalidRequest)?;
        let handle = record
            .persistence
            .as_ref()
            .ok_or_else(|| AgentManagerError::MissingPersistence(id.clone()))?;
        let session = super::execution::native(
            &spec.provider,
            "resume",
            client.resume_session(handle, &spec, AgentResumePurpose::Interactive),
        )
        .await
        .map_err(map_session)?;
        self.register_session(&id, session, record, Registration::Create, permit)
            .await
    }

    /// Restore an explicitly selected Agent for interactive use, including archived Agents.
    ///
    /// `overrides` changes only supplied fields. Native validation and session opening precede
    /// the atomic registry update; failed restore leaves the previous archive and settings intact.
    /// Existing in-flight input is never discarded to make room for a restore.
    ///
    /// # Errors
    /// Returns missing identity, busy input, invalid settings, provider or persistence failures.
    pub(crate) async fn restore(
        &mut self,
        agent_id: &str,
        overrides: &Overrides,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        if self.live.get(agent_id).is_some_and(|agent| {
            !agent.registered || agent.turn.is_some() || agent.pending.is_some()
        }) {
            return Err(AgentManagerError::Busy);
        }
        if !self.live.contains_key(agent_id) && self.live.len() >= 32 {
            return Err(AgentManagerError::Busy);
        }
        let original = self
            .registry
            .get(agent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        if original.archived_at.is_none()
            && *overrides == Overrides::default()
            && self.live.contains_key(agent_id)
        {
            return Ok(original);
        }
        validate_overrides(&original, overrides)?;
        let mut record = original;
        overrides.apply(&mut record);
        let spec = AgentSessionSpec {
            provider: record.provider.clone(),
            cwd: record.cwd.clone(),
            config: record.config.clone().unwrap_or_default(),
        };
        validate_identity(agent_id, &spec)?;
        self.available_client(&record.provider)
            .await?
            .validate_selection(&spec)
            .await
            .map_err(|_| AgentManagerError::InvalidRequest)?;
        self.close(agent_id).await?;
        let permit = self.reserve_session()?;
        // Closing may have saved a newer provider handle; resume exactly that history.
        record.persistence = self
            .registry
            .get(agent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?
            .persistence;
        let handle = record
            .persistence
            .as_ref()
            .ok_or_else(|| AgentManagerError::MissingPersistence(agent_id.to_owned()))?;
        let client = self.available_client(&record.provider).await?;
        let session = super::execution::native(
            &spec.provider,
            "restore",
            client.resume_session(handle, &spec, AgentResumePurpose::Interactive),
        )
        .await
        .map_err(map_session)?;
        self.register_session(
            agent_id,
            session,
            record,
            Registration::Restore(overrides),
            permit,
        )
        .await
    }
}

fn validate_overrides(
    record: &PersistedAgentRuntimeRecord,
    overrides: &Overrides,
) -> Result<(), AgentManagerError> {
    let handle = record
        .persistence
        .as_ref()
        .ok_or_else(|| AgentManagerError::MissingPersistence(record.id.clone()))?;
    if handle.provider != record.provider
        || overrides
            .provider
            .as_ref()
            .is_some_and(|provider| provider != &record.provider)
        || matches!(&overrides.title, NullableSetting::Set(title)
            if title.trim().is_empty() || title.trim().encode_utf16().count() > 200)
    {
        return Err(AgentManagerError::InvalidRequest);
    }
    Ok(())
}
