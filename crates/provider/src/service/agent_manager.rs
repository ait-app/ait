//! Provider session registration and recovery for the independent server.

pub(crate) mod auto_archive;
mod controls;
mod delivery;
mod execution;
mod generated_titles;
pub(crate) mod native_sessions;
pub(crate) mod ownership;
mod realtime;
mod recovery;
mod resume;
mod streaming;
mod titles;
mod usage;

use std::collections::BTreeMap;
use std::sync::Arc;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{
    AgentPersistenceHandle, AgentRuntimeStatus, PersistedAgentRuntimeRecord, StoredAgentConfig,
    StoredAgentRuntimeInfo,
};
use domain::session::protocol::SessionEventKind;
use model::session::SessionEvents;
use serde_json::json;

use crate::ports::agent_session::{
    AgentClient, AgentResumePurpose, AgentSession, AgentSessionError, AgentSessionSpec,
    AgentTurnEvent, ConfigApplication,
};
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
use crate::support::time::now_timestamp;

/// Whether accepted configuration is native-confirmed or saved for a later turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigurationOutcome {
    /// The native adapter confirmed the configuration immediately.
    Applied,
    /// The configuration is accepted for a later turn.
    Deferred,
}

/// Adapter observations authorized for one configuration write.
#[derive(Debug)]
struct ConfigurationUpdate {
    outcome: ConfigurationOutcome,
    runtime: Option<StoredAgentRuntimeInfo>,
    effective: Option<StoredAgentConfig>,
    persistence: Option<AgentPersistenceHandle>,
}

enum Registration<'a> {
    Create,
    Read,
    Configuration,
    Recovery,
    Restore(&'a crate::protocol::resume::Overrides),
}

impl Registration<'_> {
    fn update_turn_facts(&self, record: &mut PersistedAgentRuntimeRecord, mode_id: Option<&str>) {
        if matches!(self, Self::Configuration) {
            return;
        }
        record.last_mode_id = mode_id.map(str::to_owned);
        record.last_status = if matches!(self, Self::Recovery) {
            AgentRuntimeStatus::Running
        } else {
            AgentRuntimeStatus::Idle
        };
        record.last_error = None;
    }
}

/// Application failure while managing a live Agent session.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentManagerError {
    /// An Agent ID or provider identity was invalid.
    #[error("invalid Agent session request")]
    InvalidRequest,
    /// The Agent or its durable state does not exist.
    #[error("Agent not found: {0}")]
    NotFound(String),
    /// A live or durable Agent already has this ID.
    #[error("Agent already exists: {0}")]
    AlreadyExists(String),
    /// No registered, available provider can operate this Agent.
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    /// The durable Agent has no native session identity to resume.
    #[error("Agent has no resumable provider session: {0}")]
    MissingPersistence(String),
    /// The provider adapter failed or returned inconsistent facts.
    #[error("provider session failed")]
    Session,
    /// History could not be replayed; contains only adapter-owned, payload-free context.
    #[error("{0}")]
    History(String),
    /// The provider rejected the requested session operation or configuration.
    #[error("provider rejected the Agent session operation or configuration")]
    SessionRejected,
    /// Durable Agent state could not be read or written.
    #[error("Agent runtime registry failed")]
    Registry,
    /// A turn is already active or the Agent is archived.
    #[error("Agent is busy or read-only")]
    Busy,
}

/// Metadata added when a new provider session is registered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentRegistration {
    /// Workspace placement, if any.
    pub(crate) workspace_id: Option<String>,
    /// Initial user-visible title.
    pub(crate) title: Option<String>,
    /// Public and delegation labels.
    pub(crate) labels: BTreeMap<String, String>,
    /// Whether this is an internal runtime Agent.
    pub(crate) internal: bool,
    /// Durable owner of an internal session; set before registration is published.
    pub(crate) owner: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionUse {
    Interactive,
    Configuration(AgentRuntimeStatus),
}

#[derive(Debug)]
struct LiveAgent {
    usage_cache: usage::UsageCache,
    session: Box<dyn AgentSession>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    record: PersistedAgentRuntimeRecord,
    registered: bool,
    turn: Option<String>,
    last_message: Option<String>,
    last_message_observer: Option<tokio::sync::watch::Sender<Option<String>>>,
    latest_turn: Option<String>,
    pending: Option<AgentTurnEvent>,
    pending_runtime: Option<StoredAgentRuntimeInfo>,
    // Sessions resumed only for controls are released after their observations persist.
    session_use: SessionUse,
    pending_input_at: Option<String>,
    failure_diagnostic: Option<crate::ports::diagnostics::Diagnostic>,
    failure_entry: Option<crate::protocol::timeline::NativeItem>,
    exclusive: bool,
    interruption_requested: bool,
}

impl LiveAgent {
    fn new(
        session: Box<dyn AgentSession>,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
        record: PersistedAgentRuntimeRecord,
        registered: bool,
    ) -> Self {
        Self {
            usage_cache: usage::UsageCache::default(),
            session,
            permit,
            record,
            registered,
            turn: None,
            last_message: None,
            last_message_observer: None,
            latest_turn: None,
            pending: None,
            pending_runtime: None,
            session_use: SessionUse::Interactive,
            pending_input_at: None,
            failure_diagnostic: None,
            failure_entry: None,
            exclusive: false,
            interruption_requested: false,
        }
    }
}

/// Owns live provider sessions and their durable Paseo-compatible snapshots.
///
/// Callers serialize mutations to one manager. The registry and provider operations are fallible;
/// a failed registration attempts to close the unregistered session before returning an error.
#[derive(Debug)]
pub struct AgentManager {
    registry: Arc<dyn AgentRuntimeRegistry>,
    clients: BTreeMap<String, Arc<dyn AgentClient>>,
    live: BTreeMap<String, LiveAgent>,
    pending_configuration_cleanup: BTreeMap<String, LiveAgent>,
    events: SessionEvents,
    timeline: Option<crate::storage::timeline::Timeline>,
    catalog: super::provider_catalog::Catalog,
    creations: model::creation::Creations,
    loaded_timelines: Arc<std::sync::Mutex<std::collections::BTreeSet<String>>>,
    session_budget: Arc<tokio::sync::Semaphore>,
    history_budget: Arc<tokio::sync::Semaphore>,
    owner: Option<ownership::Owner>,
    generated_titles: generated_titles::Titles,
    workspace_names: Option<Arc<dyn model::workspace::lifecycle::WorkspaceNaming>>,
    auto_archives: auto_archive::AutoArchives,
    usage_cache: usage::UsageCache,
}

impl AgentManager {
    pub(crate) fn validate_provider(&self, provider: &str) -> Result<(), AgentManagerError> {
        if self.clients.contains_key(provider) {
            Ok(())
        } else {
            Err(AgentManagerError::ProviderUnavailable(provider.to_owned()))
        }
    }

    /// Compose the manager with a durable Agent runtime registry.
    #[must_use]
    pub fn new(registry: Box<dyn AgentRuntimeRegistry>) -> Self {
        Self {
            registry: registry.into(),
            clients: BTreeMap::new(),
            live: BTreeMap::new(),
            pending_configuration_cleanup: BTreeMap::new(),
            events: SessionEvents::default(),
            timeline: None,
            catalog: super::provider_catalog::Catalog::default(),
            creations: model::creation::Creations::default(),
            loaded_timelines: Arc::default(),
            session_budget: Arc::new(tokio::sync::Semaphore::new(32)),
            history_budget: Arc::new(tokio::sync::Semaphore::new(8)),
            owner: None,
            generated_titles: generated_titles::Titles::default(),
            workspace_names: None,
            auto_archives: auto_archive::AutoArchives::default(),
            usage_cache: usage::UsageCache::default(),
        }
    }

    /// Install the same creation receipt service used by Workspace metadata.
    #[must_use]
    pub fn with_creations(mut self, creations: model::creation::Creations) -> Self {
        self.creations = creations;
        self
    }

    /// Return the shared creation coordinator shared with this manager.
    #[must_use]
    pub(crate) fn creations(&self) -> model::creation::Creations {
        self.creations.clone()
    }

    /// Install durable display history before admitting requests or creating native sessions.
    #[must_use]
    pub fn with_timeline(mut self, timeline: crate::storage::timeline::Timeline) -> Self {
        self.timeline = Some(timeline);
        self
    }

    /// Install shared auxiliary generation; foreground sessions retain their own lifecycle.
    #[must_use]
    pub fn with_summary_generation(
        mut self,
        generator: Arc<dyn model::summary::SummaryGenerator>,
    ) -> Self {
        self.generated_titles.generator = Some(generator);
        self
    }

    /// Share workspace naming for workspaces created before their first Agent prompt arrives.
    #[must_use]
    pub fn with_workspace_names(
        mut self,
        names: Arc<dyn model::workspace::lifecycle::WorkspaceNaming>,
    ) -> Self {
        self.workspace_names = Some(names);
        self
    }

    /// Return the installed display projection, independent of live session ownership.
    #[must_use]
    pub(crate) fn timeline(&self) -> Option<crate::storage::timeline::Timeline> {
        self.timeline.clone()
    }

    /// Discover models and capabilities through the registered native adapters.
    /// # Errors
    /// Returns invalid requests, unavailable adapter or safe discovery/cache errors.
    #[cfg(test)]
    pub(crate) async fn providers(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, model::ErrorCode> {
        self.catalog
            .execute(&self.clients, &self.events, method, params)
            .await
    }

    /// Load native history without claiming a writer, once per Agent in this process.
    /// # Errors
    /// Returns unavailable history, missing Agent, or durable projection failures.
    pub(crate) async fn load_timeline(&mut self, agent_id: &str) -> Result<(), model::ErrorCode> {
        use model::ErrorCode;
        self.load_timeline_for_input(agent_id)
            .await
            .map_err(|error| match error {
                AgentManagerError::NotFound(_) => ErrorCode::AgentNotFound,
                AgentManagerError::ProviderUnavailable(_)
                | AgentManagerError::MissingPersistence(_) => ErrorCode::UnsupportedCapability,
                _ => ErrorCode::AgentIo,
            })
    }

    async fn load_timeline_for_input(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        if self.history_loaded(agent_id) {
            return Ok(());
        }
        let timeline = self
            .timeline
            .as_ref()
            .ok_or_else(|| AgentManagerError::ProviderUnavailable("timeline".to_owned()))?;
        let record = self
            .registry
            .get(agent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        let generation = timeline
            .generation(agent_id)
            .map_err(|_| AgentManagerError::Registry)?;
        let handle = record
            .persistence
            .as_ref()
            .ok_or_else(|| AgentManagerError::MissingPersistence(agent_id.to_owned()))?;
        if handle
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(controls::REPLACEMENT_MARKER))
            == Some(&json!(true))
        {
            let history = self
                .inspect_native(handle, &record.cwd)
                .await
                .map_err(|_| AgentManagerError::Session)?;
            self.verify_history(&record, &generation)?;
            self.finish_replacement(agent_id, &history)
                .map_err(|_| AgentManagerError::Registry)?;
            self.mark_history_loaded(agent_id);
            return Ok(());
        }
        let client = self
            .clients
            .get(&record.provider)
            .ok_or_else(|| AgentManagerError::ProviderUnavailable(record.provider.clone()))?;
        if !client.supports_history_replay() {
            self.mark_history_loaded(agent_id);
            return Ok(());
        }
        let _history = self
            .history_budget
            .acquire()
            .await
            .map_err(|_| AgentManagerError::Session)?;
        let entries = execution::native(
            &record.provider,
            "history",
            client.history(handle, &record.cwd),
        )
        .await
        .map_err(map_session)?;
        self.verify_history(&record, &generation)?;
        timeline
            .reconcile(agent_id, &record.provider, &entries)
            .map_err(|_| AgentManagerError::Registry)?;
        if titles::missing(&record) {
            self.fill_missing_title(agent_id, titles::from_entries(&entries))
                .map_err(|_| AgentManagerError::Registry)?;
        }
        self.mark_history_loaded(agent_id);
        Ok(())
    }

    /// Return the shared connection event service used for committed attention notifications.
    #[must_use]
    pub(crate) fn events(&self) -> SessionEvents {
        self.events.clone()
    }

    /// Register one provider client. A provider identity can only be registered once.
    ///
    /// # Errors
    /// Returns `InvalidRequest` for an empty identity and `AlreadyExists` for a duplicate.
    pub(crate) fn register_client(
        &mut self,
        client: Box<dyn AgentClient>,
    ) -> Result<(), AgentManagerError> {
        let provider = client.provider().to_owned();
        if provider.trim().is_empty() || provider.chars().any(char::is_control) {
            return Err(AgentManagerError::InvalidRequest);
        }
        if self.clients.contains_key(&provider) {
            return Err(AgentManagerError::AlreadyExists(provider));
        }
        self.clients.insert(provider, client.into());
        Ok(())
    }

    /// Return the live session's latest registered snapshot, if this process owns it.
    #[must_use]
    pub(crate) fn live_snapshot(&self, agent_id: &str) -> Option<&PersistedAgentRuntimeRecord> {
        self.live
            .get(agent_id)
            .filter(|agent| agent.registered)
            .map(|agent| &agent.record)
    }

    /// Return the currently accepted native turn identity, when one is active.
    #[must_use]
    pub(crate) fn active_turn(&self, agent_id: &str) -> Option<&str> {
        self.live
            .get(agent_id)
            .and_then(|agent| agent.turn.as_deref())
    }

    /// Whether an acknowledged interruption still awaits the active turn's terminal event.
    pub(crate) fn interruption_pending(&self, agent_id: &str) -> bool {
        self.live
            .get(agent_id)
            .is_some_and(|agent| agent.turn.is_some() && agent.interruption_requested)
    }

    /// Return the most recently accepted native turn, including after it finishes.
    /// Used to fence connection-owned cancellation and completion reads against later turns.
    #[must_use]
    pub(crate) fn latest_turn(&self, agent_id: &str) -> Option<&str> {
        self.live
            .get(agent_id)
            .and_then(|agent| agent.latest_turn.as_deref())
    }

    /// Return the final assistant text from this process's latest completed turn.
    #[must_use]
    pub(crate) fn last_message(&self, agent_id: &str) -> Option<&str> {
        self.live
            .get(agent_id)
            .and_then(|agent| agent.last_message.as_deref())
    }

    /// Retain this live turn's final text for an in-flight wait, even after native close.
    pub(crate) fn observe_last_message(
        &mut self,
        agent_id: &str,
    ) -> Option<tokio::sync::watch::Receiver<Option<String>>> {
        let agent = self.live.get_mut(agent_id)?;
        let sender = agent
            .last_message_observer
            .get_or_insert_with(|| tokio::sync::watch::channel(agent.last_message.clone()).0);
        Some(sender.subscribe())
    }

    /// Apply live provider controls or persist a validated patch for subsequent turns.
    ///
    /// Returns the adapter's explicit immediate or deferred disposition.
    /// # Errors
    /// Returns validation, unavailable-provider, archived-Agent, or registry errors. All supplied
    /// fields persist together. Native controls may take effect before a storage failure;
    /// subsequent provider observations retain their effective state.
    pub(crate) async fn configure(
        &mut self,
        agent_id: &str,
        patch: &ConfigPatch,
    ) -> Result<ConfigurationOutcome, AgentManagerError> {
        if self.pending_configuration_cleanup.contains_key(agent_id)
            || self
                .live
                .get(agent_id)
                .is_some_and(|agent| matches!(agent.session_use, SessionUse::Configuration(_)))
        {
            self.release_configuration_session(agent_id).await?;
        }
        let mut record = self
            .registry
            .get(agent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        let mut config = self.validate_config_patch(&record, patch).await?;
        let temporary = !self.live.contains_key(agent_id)
            && self
                .clients
                .get(&record.provider)
                .is_some_and(|client| client.supports_live_config());
        let result = async {
            if temporary {
                // Restored directory records are not live native sessions. Do not accept a
                // deferred patch when this provider promises immediate native readback.
                record = self
                    .resume_with_purpose(
                        agent_id,
                        false,
                        AgentResumePurpose::Configuration {
                            replace_model: matches!(patch.model_id, NullableSetting::Set(_)),
                            replace_thinking: patch.thinking_option_id
                                != NullableSetting::Unchanged,
                        },
                    )
                    .await?;
                config = patch.apply(
                    record
                        .config
                        .as_ref()
                        .unwrap_or(&StoredAgentConfig::default()),
                );
            }
            self.apply_validated_config(agent_id, &config, patch).await
        }
        .await;
        if temporary && self.live.contains_key(agent_id) {
            let released = self.release_configuration_session(agent_id).await;
            return match result {
                Ok(applied) => released.map(|()| applied),
                Err(error) => {
                    if let Err(release_error) = released {
                        tracing::warn!(agent_id, error_kind = %release_error,
                            "Configuration session cleanup will retry");
                    }
                    Err(error)
                }
            };
        }
        result
    }

    async fn apply_validated_config(
        &mut self,
        agent_id: &str,
        config: &StoredAgentConfig,
        patch: &ConfigPatch,
    ) -> Result<ConfigurationOutcome, AgentManagerError> {
        let ConfigurationUpdate {
            outcome,
            runtime,
            effective,
            persistence,
        } = if let Some(agent) = self.live.get_mut(agent_id) {
            let application = match agent.session.apply_config(config, patch).await {
                Ok(application) => application,
                Err(error) => {
                    // Rejection may consume queued intent or follow a successful model
                    // command. Retain its cached readback before shutdown can save a handle.
                    if let Some(info) = agent.session.take_runtime_observation() {
                        agent.pending_runtime = Some(info.clone());
                        if matches!(agent.pending, Some(AgentTurnEvent::RuntimeInfo(_))) {
                            agent.pending = Some(AgentTurnEvent::RuntimeInfo(info));
                        }
                    }
                    return Err(map_session(error));
                }
            };
            let update = match application {
                ConfigApplication::Deferred => ConfigurationUpdate {
                    outcome: ConfigurationOutcome::Deferred,
                    runtime: None,
                    effective: None,
                    persistence: None,
                },
                ConfigApplication::Queued => ConfigurationUpdate {
                    outcome: ConfigurationOutcome::Deferred,
                    runtime: None,
                    effective: agent.session.effective_config(),
                    persistence: agent.session.persistence(),
                },
                ConfigApplication::Applied(info) => ConfigurationUpdate {
                    outcome: ConfigurationOutcome::Applied,
                    runtime: Some(info),
                    effective: agent.session.effective_config(),
                    persistence: agent.session.persistence(),
                },
            };
            if update.runtime.as_ref().is_some_and(|info| {
                !valid_runtime_info(info, agent.session.provider(), &agent.record.provider)
            }) {
                return Err(AgentManagerError::Session);
            }
            if let Some(info) = &update.runtime {
                agent.pending_runtime = Some(info.clone());
                if matches!(agent.pending, Some(AgentTurnEvent::RuntimeInfo(_))) {
                    agent.pending = Some(AgentTurnEvent::RuntimeInfo(info.clone()));
                }
            }
            update
        } else {
            ConfigurationUpdate {
                outcome: ConfigurationOutcome::Deferred,
                runtime: None,
                effective: None,
                persistence: None,
            }
        };
        let now = now_timestamp();
        let updated = self
            .registry
            .update(agent_id, &|current| {
                let mut next = current.clone();
                next.config = Some(
                    patch.apply(
                        current
                            .config
                            .as_ref()
                            .unwrap_or(&StoredAgentConfig::default()),
                    ),
                );
                if let Some(effective) = &effective {
                    next.config = Some(effective.clone());
                }
                if let Some(runtime) = &runtime {
                    let mut info = runtime.clone();
                    streaming::preserve_usage(&mut info, current.runtime_info.as_ref());
                    next.runtime_info = Some(info);
                }
                if let Some(persistence) = &persistence {
                    next.persistence = Some(persistence.clone());
                }
                if patch.mode_id != NullableSetting::Unchanged {
                    next.last_mode_id
                        .clone_from(&effective.as_ref().unwrap_or(config).mode_id);
                }
                next.updated_at.clone_from(&now);
                next
            })
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        if let Some(agent) = self.live.get_mut(agent_id) {
            agent.record = updated;
            if runtime.is_some() {
                agent.pending_runtime = None;
            }
        }
        Ok(outcome)
    }

    async fn release_configuration_sessions(&mut self) {
        let ids: Vec<_> = self
            .live
            .iter()
            .filter(|(_, agent)| matches!(agent.session_use, SessionUse::Configuration(_)))
            .map(|(id, _)| id.clone())
            .chain(self.pending_configuration_cleanup.keys().cloned())
            .collect();
        for id in ids {
            if let Err(error) = self.release_configuration_session(&id).await {
                tracing::warn!(agent_id = %id, error_kind = %error,
                    "provider.configuration.release_pending");
            }
        }
    }

    async fn release_configuration_session(
        &mut self,
        agent_id: &str,
    ) -> Result<(), AgentManagerError> {
        let Some(mut agent) = self
            .live
            .remove(agent_id)
            .or_else(|| self.pending_configuration_cleanup.remove(agent_id))
        else {
            return Ok(());
        };
        // Closing invalidates the native session even when cleanup fails. Keep retryable
        // observations outside live admission and release both forms of session capacity.
        agent.permit.take();
        let result = self
            .finish_configuration_cleanup(agent_id, &mut agent)
            .await;
        if result.is_err() {
            self.pending_configuration_cleanup
                .insert(agent_id.to_owned(), agent);
        }
        result
    }

    async fn finish_configuration_cleanup(
        &self,
        agent_id: &str,
        agent: &mut LiveAgent,
    ) -> Result<(), AgentManagerError> {
        let status = match agent.session_use {
            SessionUse::Configuration(status) => status,
            SessionUse::Interactive => agent.record.last_status,
        };
        agent.session.close().await.map_err(map_session)?;
        if agent.registered {
            persist_runtime(self.registry.as_ref(), agent_id, agent)?;
            streaming::drain(
                self.registry.as_ref(),
                self.timeline.as_ref(),
                &self.events,
                agent_id,
                agent,
            )
            .await?;
        }
        self.finish_close(agent_id, agent, status).await
    }

    async fn validate_config_patch(
        &self,
        record: &PersistedAgentRuntimeRecord,
        patch: &ConfigPatch,
    ) -> Result<StoredAgentConfig, AgentManagerError> {
        if record.archived_at.is_some() {
            return Err(AgentManagerError::Busy);
        }
        if matches!(patch.mode_id, NullableSetting::Clear) {
            return Err(AgentManagerError::InvalidRequest);
        }
        let config = patch.apply(
            record
                .config
                .as_ref()
                .unwrap_or(&StoredAgentConfig::default()),
        );
        if let Some(agent) = self.live.get(&record.id) {
            agent
                .session
                .validate_config_update(&config)
                .map_err(|error| match error {
                    AgentSessionError::Busy => AgentManagerError::Busy,
                    AgentSessionError::Rejected
                    | AgentSessionError::Unavailable
                    | AgentSessionError::Failed
                    | AgentSessionError::History(_) => AgentManagerError::InvalidRequest,
                })?;
        }
        self.clients
            .get(&record.provider)
            .ok_or_else(|| AgentManagerError::ProviderUnavailable(record.provider.clone()))?
            .validate_config_patch(
                &AgentSessionSpec {
                    provider: record.provider.clone(),
                    cwd: record.cwd.clone(),
                    config: config.clone(),
                },
                patch,
            )
            .await
            .map_err(|_| AgentManagerError::InvalidRequest)?;
        Ok(config)
    }

    /// Create a native session and persist its initial Agent snapshot.
    ///
    /// # Errors
    /// Rejects invalid or duplicate IDs, unavailable providers, session failures, and storage
    /// failures. If registration fails after creation, the new session is closed.
    #[cfg(test)]
    pub(crate) async fn create(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
        registration: AgentRegistration,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.create_with_environment(
            agent_id,
            spec,
            registration,
            &crate::ports::environment::AgentEnvironment::default(),
        )
        .await
    }

    /// Create a native session with private per-process environment overrides.
    ///
    /// Environment values remain in the native adapter's memory and are never put in the registry.
    /// # Errors
    /// Returns identity, admission, native launch or registration failures with cleanup.
    pub(crate) async fn create_with_environment(
        &mut self,
        agent_id: &str,
        spec: &AgentSessionSpec,
        registration: AgentRegistration,
        environment: &crate::ports::environment::AgentEnvironment,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        validate_identity(agent_id, spec)?;
        if self.live.len() >= 32 {
            return Err(AgentManagerError::Busy);
        }
        if self.live.contains_key(agent_id)
            || self.registry.get(agent_id).map_err(map_registry)?.is_some()
        {
            return Err(AgentManagerError::AlreadyExists(agent_id.to_owned()));
        }
        let permit = self.reserve_session()?;
        let client = self.available_client(&spec.provider).await?;
        let session = execution::native(
            &spec.provider,
            "create",
            client.create_session_with_environment(spec, environment),
        )
        .await
        .map_err(map_session)?;
        let now = now_timestamp();
        let record = PersistedAgentRuntimeRecord {
            id: agent_id.to_owned(),
            provider: spec.provider.clone(),
            cwd: spec.cwd.clone(),
            workspace_id: registration.workspace_id,
            created_at: now.clone(),
            updated_at: now,
            last_activity_at: None,
            last_user_message_at: None,
            title: registration.title,
            title_origin: None,
            labels: registration.labels,
            last_status: AgentRuntimeStatus::Idle,
            last_mode_id: spec.config.mode_id.clone(),
            config: Some(spec.config.clone()),
            runtime_info: None,
            features: Vec::new(),
            persistence: None,
            last_error: None,
            requires_attention: false,
            attention_reason: None,
            attention_timestamp: None,
            internal: registration.internal,
            archived_at: None,
            owner: registration.owner,
        };
        let record = self
            .register_session(
                agent_id,
                session,
                record,
                Registration::Create,
                Some(permit),
            )
            .await?;
        self.mark_history_loaded(agent_id);
        Ok(record)
    }

    /// Resume a persisted provider session, using history-only mode for archived Agents.
    ///
    /// Restoring a session does not change its durable activity and update timestamps. A record
    /// without a provider handle cannot be resumed and is left untouched.
    ///
    /// # Errors
    /// Returns missing-state, unavailable-provider, provider, or registry failures.
    pub(crate) async fn resume(
        &mut self,
        agent_id: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.resume_with_recovery(agent_id, false).await
    }

    async fn resume_with_recovery(
        &mut self,
        agent_id: &str,
        recovering: bool,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        self.resume_with_purpose(agent_id, recovering, AgentResumePurpose::Interactive)
            .await
    }

    async fn resume_with_purpose(
        &mut self,
        agent_id: &str,
        recovering: bool,
        purpose: AgentResumePurpose,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        if self.pending_configuration_cleanup.contains_key(agent_id)
            || self
                .live
                .get(agent_id)
                .is_some_and(|agent| matches!(agent.session_use, SessionUse::Configuration(_)))
        {
            self.release_configuration_session(agent_id).await?;
        }
        if let Some(agent) = self.live.get(agent_id) {
            return if agent.registered {
                self.registry
                    .get(agent_id)
                    .map_err(map_registry)?
                    .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))
            } else {
                Err(AgentManagerError::Session)
            };
        }
        let permit = if matches!(purpose, AgentResumePurpose::Configuration { .. }) {
            None
        } else {
            if self.live.len() >= 32 {
                return Err(AgentManagerError::Busy);
            }
            Some(self.reserve_session()?)
        };
        let mut record = self
            .registry
            .get(agent_id)
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        if record
            .persistence
            .as_ref()
            .and_then(|handle| handle.metadata.as_ref())
            .and_then(|metadata| metadata.get(controls::REPLACEMENT_MARKER))
            == Some(&json!(true))
        {
            self.load_timeline_for_input(agent_id).await?;
            record = self
                .registry
                .get(agent_id)
                .map_err(map_registry)?
                .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        }
        let handle = record
            .persistence
            .as_ref()
            .ok_or_else(|| AgentManagerError::MissingPersistence(agent_id.to_owned()))?;
        if handle.provider != record.provider {
            return Err(AgentManagerError::InvalidRequest);
        }
        let spec = AgentSessionSpec {
            provider: record.provider.clone(),
            cwd: record.cwd.clone(),
            config: record.config.clone().unwrap_or_default(),
        };
        validate_identity(agent_id, &spec)?;
        let purpose = if record.archived_at.is_some() {
            AgentResumePurpose::History
        } else {
            purpose
        };
        let client = self.available_client(&record.provider).await?;
        let session = execution::native(
            &spec.provider,
            "resume",
            client.resume_session(handle, &spec, purpose),
        )
        .await
        .map_err(map_session)?;
        let registration = if matches!(purpose, AgentResumePurpose::Configuration { .. }) {
            Registration::Configuration
        } else if recovering {
            Registration::Recovery
        } else {
            Registration::Read
        };
        self.register_session(agent_id, session, record, registration, permit)
            .await
    }

    /// Close one live session while retaining its provider history and durable Agent record.
    ///
    /// A native close failure retains manager ownership so a later attempt can retry. Missing
    /// live sessions are idempotent.
    ///
    /// # Errors
    /// Returns provider or registry failure.
    pub(crate) async fn close(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        self.close_with_status(agent_id, AgentRuntimeStatus::Closed)
            .await
    }

    async fn close_with_status(
        &mut self,
        agent_id: &str,
        status: AgentRuntimeStatus,
    ) -> Result<(), AgentManagerError> {
        if self.pending_configuration_cleanup.contains_key(agent_id) {
            self.release_configuration_session(agent_id).await?;
        }
        let Some(mut agent) = self.live.remove(agent_id) else {
            return Ok(());
        };
        let result = self.finish_close(agent_id, &mut agent, status).await;
        if result.is_err() {
            self.live.insert(agent_id.to_owned(), agent);
        }
        result
    }

    async fn finish_close(
        &self,
        agent_id: &str,
        agent: &mut LiveAgent,
        status: AgentRuntimeStatus,
    ) -> Result<(), AgentManagerError> {
        if agent.registered {
            persist_runtime(self.registry.as_ref(), agent_id, agent)?;
            streaming::persist_handle(self.registry.as_ref(), agent_id, agent)?;
        }
        agent.session.close().await.map_err(map_session)?;
        if agent.registered {
            for child in agent.session.subagents() {
                controls::publish_subagent(
                    self.timeline.as_ref(),
                    &self.events,
                    agent,
                    &crate::ports::controls::SubagentEvent::Upsert(child),
                )
                .map_err(|_| AgentManagerError::Registry)?;
            }
        }
        if !agent.registered {
            return Ok(());
        }
        // Metadata and attention may have changed through other narrow ports. Never replace
        // their latest record with the manager's creation-time snapshot or resurrect a deletion.
        let durable_permissions = agent.session.persistence().is_some_and(|handle| {
            self.clients
                .get(&agent.record.provider)
                .is_some_and(|client| !client.persisted_permissions(&handle).is_empty())
        });
        self.registry
            .update(agent_id, &|current| {
                let mut next = current.clone();
                next.last_status = status;
                if next.attention_reason
                    == Some(domain::agent_runtime::AgentAttentionReason::Permission)
                    && !durable_permissions
                {
                    next.requires_attention = false;
                    next.attention_reason = None;
                    next.attention_timestamp = None;
                }
                next
            })
            .map_err(map_registry)?;
        Ok(())
    }

    /// Start one native text turn. Busy Agents reject additional work explicitly.
    ///
    /// # Errors
    /// Returns resume, provider, storage, or busy/read-only failures.
    #[cfg(test)]
    pub(crate) async fn send(
        &mut self,
        agent_id: &str,
        text: &str,
    ) -> Result<(), AgentManagerError> {
        self.send_input(agent_id, &crate::protocol::prompt::AgentPrompt::text(text))
            .await
    }

    /// Submit one complete rich prompt while retaining exclusive native-turn admission.
    /// # Errors
    /// Returns invalid input, busy ownership, native admission or persistence failures.
    pub(crate) async fn send_input(
        &mut self,
        agent_id: &str,
        prompt: &crate::protocol::prompt::AgentPrompt,
    ) -> Result<(), AgentManagerError> {
        prompt
            .validate()
            .map_err(|_| AgentManagerError::InvalidRequest)?;
        if self.timeline.is_some() && !self.history_loaded(agent_id) {
            self.load_timeline_for_input(agent_id).await?;
        }
        let record = self.resume(agent_id).await?;
        if record.archived_at.is_some() || self.active_turn(agent_id).is_some() {
            return Err(AgentManagerError::Busy);
        }
        let now = now_timestamp();
        let restarting = prompt
            .client_message_id
            .as_deref()
            .is_some_and(|message| message.starts_with(recovery::RESTART_INPUT_PREFIX));
        self.registry
            .update(agent_id, &|current| {
                let mut next = current.clone();
                next.last_status = AgentRuntimeStatus::Running;
                next.updated_at.clone_from(&now);
                if !restarting {
                    next.last_user_message_at = Some(now.clone());
                }
                next.last_activity_at = Some(now.clone());
                next.last_error = None;
                next.requires_attention = false;
                next.attention_reason = None;
                next.attention_timestamp = None;
                if titles::missing(current) {
                    next.title = titles::from_prompt(&prompt.text);
                    next.title_origin = next
                        .title
                        .as_ref()
                        .map(|_| domain::agent_runtime::TitleOrigin::Prompt);
                }
                next
            })
            .map_err(map_registry)?
            .ok_or_else(|| AgentManagerError::NotFound(agent_id.to_owned()))?;
        let agent = self
            .live
            .get_mut(agent_id)
            .ok_or(AgentManagerError::Session)?;
        agent.last_message = None;
        agent.last_message_observer = None;
        if let Some(names) = &self.workspace_names
            && let Some(workspace) = record.workspace_id.clone()
            && !record.internal
        {
            let context = self
                .timeline
                .as_ref()
                .and_then(|timeline| timeline.first_user_text(agent_id).ok().flatten())
                .unwrap_or_else(|| prompt.text.clone());
            names.schedule(
                workspace,
                context,
                Some(domain::summary::SummarySelection {
                    provider: record.provider.clone(),
                    model: record
                        .config
                        .as_ref()
                        .and_then(|config| config.model.clone()),
                    thinking_option_id: record
                        .config
                        .as_ref()
                        .and_then(|config| config.thinking_option_id.clone()),
                }),
            );
        }
        let config = record.config.unwrap_or_default();
        if let Ok(turn) = agent.session.start_input(prompt, &config).await {
            agent.latest_turn = Some(turn.clone());
            agent.turn = Some(turn);
            match agent.session.runtime_info().await {
                Ok(info) => agent.pending_runtime = Some(info),
                Err(_) => agent.pending = Some(AgentTurnEvent::Failed),
            }
            Ok(())
        } else {
            // Admission may consume deferred controls even when native readback fails.
            // Retain cached facts before terminal failure prevents normal event draining.
            if let Some(info) = agent.session.take_runtime_observation() {
                agent.pending_runtime = Some(info);
            } else if let Ok(info) = agent.session.runtime_info().await {
                agent.pending_runtime = Some(info);
            }
            agent.pending = Some(AgentTurnEvent::Failed);
            self.poll().await?;
            Err(AgentManagerError::Session)
        }
    }

    /// Request interruption; completion is acknowledged only by the provider's terminal event.
    ///
    /// # Errors
    /// Returns native interruption failures. An idle Agent is an idempotent success.
    pub(crate) async fn cancel(&mut self, agent_id: &str) -> Result<(), AgentManagerError> {
        if let Some(timeline) = &self.timeline {
            timeline
                .cancel_inputs(agent_id)
                .map_err(|_| AgentManagerError::Registry)?;
        }
        if self.active_turn(agent_id).is_none() {
            self.registry
                .update(agent_id, &|current| {
                    let mut next = current.clone();
                    if next.last_status == AgentRuntimeStatus::Running {
                        next.last_status = AgentRuntimeStatus::Idle;
                    }
                    next
                })
                .map_err(map_registry)?;
        }
        let Some(agent) = self.live.get_mut(agent_id) else {
            return Ok(());
        };
        if let Some(turn) = &agent.turn {
            agent.session.cancel_turn(turn).await.map_err(map_session)?;
            agent.interruption_requested = true;
        }
        agent.session.cancel_pending().await.map_err(map_session)?;
        Ok(())
    }

    /// Persist queued native terminal events before exposing completion to clients.
    ///
    /// A failed write retains its event for retry. Native failures close the process before
    /// publishing an error, so unsupported interactions cannot keep executing invisibly.
    /// Temporary configuration cleanup failures are logged and retried independently so they
    /// do not prevent command dispatch or cleanup of other sessions.
    ///
    /// # Errors
    /// Returns the first storage or close failure; later calls retry pending work.
    pub(crate) async fn poll(&mut self) -> Result<(), AgentManagerError> {
        self.release_configuration_sessions().await;
        let ids = self.live.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let Some(agent) = self.live.get_mut(&id) else {
                continue;
            };
            if !agent.registered || matches!(agent.session_use, SessionUse::Configuration(_)) {
                continue;
            }
            persist_runtime(self.registry.as_ref(), &id, agent)?;
            streaming::drain(
                self.registry.as_ref(),
                self.timeline.as_ref(),
                &self.events,
                &id,
                agent,
            )
            .await?;
            streaming::persist_handle(self.registry.as_ref(), &id, agent)?;
            if matches!(agent.pending, Some(AgentTurnEvent::Failed)) {
                streaming::settle_failure(self.timeline.as_ref(), &self.events, &id, agent).await?;
            }
            let Some(event) = &agent.pending else {
                continue;
            };
            let failed = matches!(event, AgentTurnEvent::Failed);
            let cancelled = matches!(event, AgentTurnEvent::Cancelled);
            let permission = !agent.session.pending_permissions().is_empty();
            let queued = self
                .timeline
                .as_ref()
                .map(|timeline| timeline.has_queued_input(&id))
                .transpose()
                .map_err(|_| AgentManagerError::Registry)?
                .unwrap_or(false);
            let now = now_timestamp();
            let committed = self
                .registry
                .update(&id, &|current| {
                    let mut next = current.clone();
                    next.last_status = if queued {
                        AgentRuntimeStatus::Running
                    } else if failed {
                        AgentRuntimeStatus::Error
                    } else {
                        AgentRuntimeStatus::Idle
                    };
                    next.last_error =
                        failed.then(|| streaming::failure_message(agent).into_owned());
                    next.updated_at.clone_from(&now);
                    next.last_activity_at = Some(now.clone());
                    next.requires_attention = permission || (!cancelled && !queued);
                    next.attention_reason = next.requires_attention.then_some(if permission {
                        domain::agent_runtime::AgentAttentionReason::Permission
                    } else if failed {
                        domain::agent_runtime::AgentAttentionReason::Error
                    } else {
                        domain::agent_runtime::AgentAttentionReason::Finished
                    });
                    next.attention_timestamp = next.requires_attention.then(|| now.clone());
                    next
                })
                .map_err(map_registry)?;
            if let AgentTurnEvent::Completed(message) = event {
                agent.last_message.clone_from(message);
            }
            streaming::publish_terminal(
                self.timeline.as_ref(),
                &id,
                agent,
                event,
                committed.as_ref(),
            );
            agent.turn = None;
            agent.pending = None;
            agent.exclusive = false;
            agent.interruption_requested = false;
            if !permission
                && !cancelled
                && !queued
                && committed
                    .as_ref()
                    .is_some_and(|record| record.archived_at.is_none() && !record.internal)
            {
                publish_terminal_attention(&self.events, &id, &now, failed);
            }
            if let Some(record) = committed
                .as_ref()
                .filter(|record| failed && !record.internal)
            {
                publish_failure_activity(&self.events, &id, &now, record);
            }
            if failed {
                self.live.remove(&id);
            }
            self.auto_archives.completed(&id);
        }
        self.archive_finished().await
    }

    /// Close writers whose durable records were archived or removed by metadata operations.
    ///
    /// # Errors
    /// Returns registry or native-close errors; ownership remains available for retry.
    pub(crate) async fn reconcile(&mut self) -> Result<(), AgentManagerError> {
        let mut close = Vec::new();
        for (id, live) in &self.live {
            let current = self.registry.get(id).map_err(map_registry)?;
            if current.is_none()
                || (live.record.archived_at.is_none()
                    && current
                        .as_ref()
                        .is_some_and(|record| record.archived_at.is_some()))
            {
                close.push(id.clone());
            }
        }
        for id in close {
            self.close(&id).await?;
        }
        Ok(())
    }

    /// Close every live session. Failed closes retain ownership for retry.
    ///
    /// # Errors
    /// Returns the first provider or registry failure after attempting every close.
    pub(crate) async fn close_all(&mut self) -> Result<(), AgentManagerError> {
        self.generated_titles.close().await;
        let ids = self
            .live
            .keys()
            .chain(self.pending_configuration_cleanup.keys())
            .cloned()
            .collect::<Vec<_>>();
        let mut first_error = None;
        for id in ids {
            if let Err(error) = self.close(&id).await {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    async fn available_client(
        &self,
        provider: &str,
    ) -> Result<&dyn AgentClient, AgentManagerError> {
        let client = self
            .clients
            .get(provider)
            .ok_or_else(|| AgentManagerError::ProviderUnavailable(provider.to_owned()))?;
        if !client.is_available().await.map_err(map_session)? {
            return Err(AgentManagerError::ProviderUnavailable(provider.to_owned()));
        }
        Ok(client.as_ref())
    }

    async fn register_session(
        &mut self,
        agent_id: &str,
        mut session: Box<dyn AgentSession>,
        mut record: PersistedAgentRuntimeRecord,
        registration: Registration<'_>,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Result<PersistedAgentRuntimeRecord, AgentManagerError> {
        let mut runtime_info = match session.runtime_info().await {
            Ok(info) if valid_runtime_info(&info, session.provider(), &record.provider) => info,
            Err(_) if !matches!(registration, Registration::Create) => {
                let rejection = self.reject_registration(agent_id, session, record, permit);
                return Err(Box::pin(rejection).await);
            }
            Ok(_) | Err(_) => {
                return Err(self.reject_session(agent_id, session, record, permit).await);
            }
        };
        streaming::preserve_usage(&mut runtime_info, record.runtime_info.as_ref());
        let persistence = session.persistence().or(record.persistence.clone());
        if !valid_registration_persistence(persistence.as_ref(), &runtime_info, &record.provider) {
            return Err(self.reject_session(agent_id, session, record, permit).await);
        }
        let effective = session.effective_config();
        let has_effective_config = effective.is_some();
        if let Some(effective) = effective {
            record.config = Some(effective);
        }
        let configuration_only = matches!(registration, Registration::Configuration);
        registration.update_turn_facts(&mut record, runtime_info.mode_id.as_deref());
        record.runtime_info = Some(runtime_info);
        let previous_persistence = std::mem::replace(&mut record.persistence, persistence);
        if self.owner.as_ref().is_some_and(|owner| {
            owner
                .bind(&record)
                .and_then(|()| owner.admitting(agent_id))
                .is_err()
        }) {
            return Err(self.reject_session(agent_id, session, record, permit).await);
        }
        let stored = if matches!(registration, Registration::Create) {
            self.registry.upsert(&record).map(|()| Some(record.clone()))
        } else {
            self.registry.update(agent_id, &|current| {
                let mut next = current.clone();
                if let Registration::Restore(overrides) = &registration {
                    overrides.apply(&mut next);
                    next.updated_at = now_timestamp();
                }
                if has_effective_config {
                    next.config.clone_from(&record.config);
                }
                next.runtime_info.clone_from(&record.runtime_info);
                next.persistence.clone_from(&record.persistence);
                registration.update_turn_facts(&mut next, record.last_mode_id.as_deref());
                next
            })
        };
        let retry_initialization = stored.is_err()
            && !matches!(registration, Registration::Create)
            && (session.initialization_error().is_some()
                || record.persistence != previous_persistence);
        if let Ok(Some(current)) = stored {
            record = current;
        } else {
            let close_result = session.close().await;
            if retry_initialization {
                // Initialization changed durable native state before registration failed,
                // including successfully consumed controls. Retain the readback and old
                // durable handle so cleanup retries both writes without native replay.
                record.persistence = previous_persistence;
                let mut agent = LiveAgent::new(session, permit, record, true);
                agent.pending_runtime.clone_from(&agent.record.runtime_info);
                agent.session_use = SessionUse::Configuration(agent.record.last_status);
                self.live.insert(agent_id.to_owned(), agent);
                return Err(if close_result.is_err() {
                    AgentManagerError::Session
                } else {
                    AgentManagerError::Registry
                });
            }
            if close_result.is_err() {
                self.live.insert(
                    agent_id.to_owned(),
                    LiveAgent::new(session, permit, record, false),
                );
                return Err(AgentManagerError::Session);
            }
            return Err(AgentManagerError::Registry);
        }
        let initialization_error = session.initialization_error();
        let mut agent = LiveAgent::new(session, permit, record.clone(), true);
        if configuration_only {
            agent.session_use = SessionUse::Configuration(record.last_status);
        }
        self.live.insert(agent_id.to_owned(), agent);
        if let Some(error) = initialization_error {
            self.close_with_status(agent_id, record.last_status).await?;
            return Err(map_session(error));
        }
        Ok(record)
    }

    async fn reject_registration(
        &mut self,
        agent_id: &str,
        mut session: Box<dyn AgentSession>,
        record: PersistedAgentRuntimeRecord,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> AgentManagerError {
        if let Some(info) = session.take_runtime_observation()
            && valid_runtime_info(&info, session.provider(), &record.provider)
            && valid_registration_persistence(
                session.persistence().as_ref(),
                &info,
                &record.provider,
            )
        {
            // Initialization may already have consumed deferred controls. Preserve
            // verified facts through retryable cleanup even if fresh readback fails.
            let mut agent = LiveAgent::new(session, permit, record, true);
            agent.pending_runtime = Some(info);
            agent.session_use = SessionUse::Configuration(agent.record.last_status);
            self.live.insert(agent_id.to_owned(), agent);
            return self
                .release_configuration_session(agent_id)
                .await
                .err()
                .unwrap_or(AgentManagerError::Session);
        }
        self.reject_session(agent_id, session, record, permit).await
    }

    async fn reject_session(
        &mut self,
        agent_id: &str,
        mut session: Box<dyn AgentSession>,
        record: PersistedAgentRuntimeRecord,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> AgentManagerError {
        if session.close().await.is_err() {
            self.live.insert(
                agent_id.to_owned(),
                LiveAgent::new(session, permit, record, false),
            );
        }
        AgentManagerError::Session
    }
}

fn valid_registration_persistence(
    persistence: Option<&AgentPersistenceHandle>,
    info: &StoredAgentRuntimeInfo,
    provider: &str,
) -> bool {
    persistence.is_none_or(|handle| {
        handle.provider == provider
            && info
                .session_id
                .as_ref()
                .is_none_or(|id| id == &handle.session_id)
    })
}

fn valid_runtime_info(
    info: &StoredAgentRuntimeInfo,
    session_provider: &str,
    expected: &str,
) -> bool {
    info.provider == expected && session_provider == expected
}

fn validate_identity(agent_id: &str, spec: &AgentSessionSpec) -> Result<(), AgentManagerError> {
    let fields = [agent_id, spec.provider.as_str(), spec.cwd.as_str()];
    if agent_id.len() > 128
        || fields.iter().any(|value| {
            value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
        })
    {
        return Err(AgentManagerError::InvalidRequest);
    }
    Ok(())
}

fn publish_failure_activity(
    events: &SessionEvents,
    agent_id: &str,
    timestamp: &str,
    record: &PersistedAgentRuntimeRecord,
) {
    events.publish(
        SessionEventKind::ActivityLog,
        &json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": timestamp,
            "type": "error",
            "content": record.last_error.as_deref().unwrap_or("Provider execution failed"),
            "metadata": {"agentId": agent_id},
        }),
    );
}

fn publish_terminal_attention(
    events: &SessionEvents,
    agent_id: &str,
    timestamp: &str,
    failed: bool,
) {
    events.publish(
        SessionEventKind::AgentAttention,
        &json!({
            "agentId": agent_id,
            "reason": if failed { "error" } else { "finished" },
            "timestamp": timestamp,
        }),
    );
}

const fn map_registry(
    _: domain::agent_runtime::registry::AgentRuntimeRegistryError,
) -> AgentManagerError {
    AgentManagerError::Registry
}

fn map_session(error: AgentSessionError) -> AgentManagerError {
    match error {
        AgentSessionError::Busy => AgentManagerError::Busy,
        AgentSessionError::Rejected => AgentManagerError::SessionRejected,
        AgentSessionError::Unavailable | AgentSessionError::Failed => AgentManagerError::Session,
        AgentSessionError::History(error) => AgentManagerError::History(error.to_string()),
    }
}

#[cfg(test)]
mod tests;

fn persist_runtime(
    registry: &dyn AgentRuntimeRegistry,
    id: &str,
    agent: &mut LiveAgent,
) -> Result<(), AgentManagerError> {
    if let Some(info) = &agent.pending_runtime {
        let persistence =
            streaming::persist_info(registry, id, agent, info).map_err(|error| match error {
                model::ErrorCode::AgentNotFound => AgentManagerError::NotFound(id.to_owned()),
                model::ErrorCode::InvalidMessage => AgentManagerError::Session,
                _ => AgentManagerError::Registry,
            })?;
        if let Some(handle) = persistence {
            agent.record.persistence = Some(handle);
        }
        agent.pending_runtime = None;
    }
    Ok(())
}
