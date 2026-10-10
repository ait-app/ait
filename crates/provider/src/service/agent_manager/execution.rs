use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;

use model::ErrorCode;
use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;

use super::{AgentManager, AgentManagerError, ownership::Owner};

impl AgentManager {
    pub(super) fn verify_history(
        &self,
        expected: &domain::agent_runtime::PersistedAgentRuntimeRecord,
        generation: &str,
    ) -> Result<(), ErrorCode> {
        let current = self
            .registry
            .get(&expected.id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        if current.persistence != expected.persistence
            || current.archived_at != expected.archived_at
            || self
                .timeline
                .as_ref()
                .ok_or(ErrorCode::UnsupportedCapability)?
                .generation(&expected.id)?
                != generation
        {
            return Err(ErrorCode::AgentIo);
        }
        Ok(())
    }
    /// Share factories and durable services while retaining separate live session ownership.
    pub(crate) fn fork(&self, owner: Option<Owner>) -> Self {
        Self {
            registry: self.registry.clone(),
            clients: self.clients.clone(),
            live: BTreeMap::new(),
            events: self.events.clone(),
            timeline: self.timeline.clone(),
            catalog: self.catalog.clone(),
            creations: self.creations.clone(),
            loaded_timelines: self.loaded_timelines.clone(),
            session_budget: self.session_budget.clone(),
            history_budget: self.history_budget.clone(),
            owner,
            generated_titles: super::generated_titles::Titles::default(),
            workspace_names: self.workspace_names.clone(),
            usage_cache: self.usage_cache.clone(),
            auto_archives: super::auto_archive::AutoArchives::default(),
        }
    }

    /// Return the globally owned catalog for routing and shutdown.
    pub(crate) fn catalog(&self) -> super::super::provider_catalog::Catalog {
        self.catalog.clone()
    }

    /// Read a snapshot without holding a session lane during discovery.
    /// # Errors
    /// Returns invalid scopes, unsupported providers and discovery admission failures.
    pub(crate) async fn read_providers(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, ErrorCode> {
        self.catalog
            .read(&self.clients, &self.events, method, params)
            .await
    }

    /// Whether the native history has been reconciled in this daemon generation.
    pub(crate) fn history_loaded(&self, id: &str) -> bool {
        self.histories().contains(id)
    }

    pub(super) fn mark_history_loaded(&self, id: &str) {
        self.histories().insert(id.to_owned());
    }

    pub(super) fn invalidate_history(&self, id: &str) {
        self.histories().remove(id);
    }

    fn histories(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.loaded_timelines
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Reserve global native capacity before a factory can allocate a child process.
    pub(super) fn reserve_session(&self) -> Result<OwnedSemaphorePermit, AgentManagerError> {
        let permit = self
            .session_budget
            .clone()
            .try_acquire_owned()
            .map_err(|_| AgentManagerError::Busy)?;
        tracing::debug!(
            active = 32 - self.session_budget.available_permits(),
            "provider.session.admitted"
        );
        Ok(permit)
    }

    /// Whether this manager is the writer owner rather than an independent reader.
    pub(crate) fn owns_runtime(&self, id: &str) -> bool {
        self.owner.as_ref().is_some_and(|owner| owner.owns(id))
    }

    /// Return identities whose post-commit observations belong to this lane.
    pub(crate) fn owned_agents(&self) -> Vec<String> {
        self.owner.as_ref().map_or_else(Vec::new, Owner::agents)
    }

    /// Publish placement before starting a native child.
    /// # Errors
    /// Returns poisoned routing state failures.
    pub(crate) fn place_workspace(&self, workspace: &str) -> Result<(), ErrorCode> {
        if let Some(owner) = &self.owner {
            owner.place(workspace)?;
        }
        Ok(())
    }

    /// Forget routing aliases only after every native resource has been released.
    pub(crate) fn release_owner(&self) {
        if !self.has_sessions()
            && let Some(owner) = &self.owner
        {
            owner.release();
        }
    }

    /// Whether the lane retains native resources that must be polled or reaped.
    pub(crate) fn has_sessions(&self) -> bool {
        !self.live.is_empty()
    }
}

pub(super) async fn native<T>(
    provider: &str,
    operation: &'static str,
    future: impl Future<Output = Result<T, crate::ports::agent_session::AgentSessionError>>,
) -> Result<T, crate::ports::agent_session::AgentSessionError> {
    let started = std::time::Instant::now();
    let result = future.await;
    if let Err(error) = &result {
        tracing::warn!(provider, operation, error = ?error, "Native provider operation failed");
    }
    tracing::debug!(
        provider,
        operation,
        elapsed_ms = started.elapsed().as_millis(),
        failed = result.is_err(),
        "provider.native.operation"
    );
    result
}
