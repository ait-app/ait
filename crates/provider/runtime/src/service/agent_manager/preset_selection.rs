//! Persisted model-preset identity and reversible ownership of role settings.

use std::collections::{BTreeMap, BTreeSet};

use domain::agent_runtime::{ModelPresetReference, ModelPresetState, StoredAgentConfig};
use provider_core::ports::preset::Preset;
use provider_core::protocol::agent_config::{ConfigPatch, NullableSetting};
use serde_json::Value;

fn is_role(key: &str) -> bool {
    key.starts_with("modelRole.") || key.starts_with("modelFallback.")
}

fn roles(config: &StoredAgentConfig) -> BTreeMap<String, Value> {
    config
        .feature_values
        .iter()
        .flatten()
        .filter(|(key, _)| is_role(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Restore only fields still owned by the selected preset; independently edited fields survive.
fn restore(config: &mut StoredAgentConfig, state: &ModelPresetState) {
    let values = config.feature_values.get_or_insert_with(BTreeMap::new);
    for field in &state.owned {
        values.remove(field);
        if let Some(value) = state.baseline.get(field) {
            values.insert(field.clone(), value.clone());
        }
    }
    if values.is_empty() {
        config.feature_values = None;
    }
    config
        .thinking_option_id
        .clone_from(&state.baseline_thinking);
}

/// Apply a user patch, preserving independent roles when the user leaves or customizes a preset.
pub(super) fn manual(
    config: &StoredAgentConfig,
    state: Option<&ModelPresetState>,
    patch: &ConfigPatch,
) -> (StoredAgentConfig, Option<ModelPresetState>) {
    let mut config = config.clone();
    let mut state = state.cloned();
    let model_changed = !matches!(patch.model_id, NullableSetting::Unchanged);
    let thinking_changed = !matches!(patch.thinking_option_id, NullableSetting::Unchanged);
    let role_changed = patch
        .feature_values
        .iter()
        .flatten()
        .any(|(field, _)| is_role(field));
    if model_changed || thinking_changed || role_changed {
        if let Some(previous) = &state {
            restore(&mut config, previous);
        }
        state = None;
    }
    (patch.apply(&config), state)
}

/// Expand the current library snapshot while keeping the baseline that existed before the preset.
pub(super) fn select(
    current: &StoredAgentConfig,
    previous: Option<&ModelPresetState>,
    reference: ModelPresetReference,
    preset: &Preset,
) -> (StoredAgentConfig, ModelPresetState) {
    let mut config = current.clone();
    if let Some(previous) = previous {
        restore(&mut config, previous);
    }
    let baseline = roles(&config);
    let baseline_thinking = config.thinking_option_id.clone();
    let mut owned: BTreeSet<String> = baseline.keys().cloned().collect();
    owned.extend(
        preset
            .feature_values
            .keys()
            .filter(|key| is_role(key))
            .cloned(),
    );
    let values = config.feature_values.get_or_insert_with(BTreeMap::new);
    for field in &owned {
        values.remove(field);
    }
    values.extend(
        preset
            .feature_values
            .iter()
            .filter(|(key, _)| is_role(key) && *key != "modelRole.default")
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    if values.is_empty() {
        config.feature_values = None;
    }
    config.model = Some(preset.model.clone());
    config
        .thinking_option_id
        .clone_from(&preset.thinking_option_id);
    (
        config,
        ModelPresetState {
            selected: Some(reference),
            baseline,
            baseline_thinking,
            owned,
        },
    )
}

/// A library lookup and adapter validation performed outside the session lane.
pub(crate) struct PreparedPreset {
    id: String,
    expected_revision: u64,
    expected_created_at: String,
    expected_provider: String,
    expected_cwd: String,
    store_revision: u64,
    config: StoredAgentConfig,
    state: ModelPresetState,
}

fn source_name(source: provider_core::ports::preset::PresetSource) -> &'static str {
    use provider_core::ports::preset::PresetSource;
    match source {
        PresetSource::Ait => "ait",
        PresetSource::Global => "global",
        PresetSource::Project => "project",
    }
}

impl super::AgentManager {
    fn preset_lookup_record(
        &self,
        id: &str,
        name: &str,
        source: &str,
        scope: Option<&ModelPresetReference>,
    ) -> Result<(domain::agent_runtime::PersistedAgentRuntimeRecord, String), model::ErrorCode>
    {
        use model::ErrorCode;
        if !domain::preset::valid_text(name) || !matches!(source, "ait" | "global" | "project") {
            return Err(ErrorCode::InvalidMessage);
        }
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        if record.observation.is_some() {
            return Err(ErrorCode::UnsupportedCapability);
        }
        if record.archived_at.is_some() || record.config_revision == u64::MAX {
            return Err(ErrorCode::CatalogBusy);
        }
        if scope.is_some_and(|scope| {
            scope.agent_cwd != record.cwd || scope.name != name || scope.source != source
        }) {
            return Err(ErrorCode::InvalidMessage);
        }
        let selected = record
            .model_preset
            .as_ref()
            .and_then(|state| state.selected.as_ref());
        if selected.is_some_and(|scope| scope.agent_cwd != record.cwd) {
            return Err(ErrorCode::InvalidMessage);
        }
        let source_cwd = scope
            .or(selected)
            .map_or_else(|| record.cwd.clone(), |scope| scope.cwd.clone());
        Ok((record, source_cwd))
    }

    /// Resolve a preset on an independent task, leaving this session available for events.
    pub(crate) fn prepare_preset(
        &self,
        id: &str,
        name: &str,
        source: &str,
        scope: Option<&ModelPresetReference>,
    ) -> Result<
        tokio::sync::oneshot::Receiver<Result<PreparedPreset, model::ErrorCode>>,
        model::ErrorCode,
    > {
        use model::ErrorCode;
        let (record, source_cwd) = self.preset_lookup_record(id, name, source, scope)?;
        let service = self
            .presets
            .clone()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let client = self
            .clients
            .get(&record.provider)
            .cloned()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let mut effective = record.config.clone().unwrap_or_default();
        if let Some(binding) = self.live.get(id).and_then(|agent| agent.mcp.as_ref()) {
            binding.apply(&mut effective);
        }
        let store_revision = service.revision();
        let name = name.to_owned();
        let source = source.to_owned();
        let (mut sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            #[cfg(test)]
            service.signal_lookup_started();
            #[cfg(test)]
            let notification = service.clone();
            let work = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                let _guard = tokio::time::timeout(
                    std::time::Duration::from_secs(60),
                    service.fence().read_owned(),
                )
                .await
                .map_err(|_| ErrorCode::CatalogBusy)?;
                if service.revision() != store_revision {
                    return Err(ErrorCode::CatalogBusy);
                }
                let entries = service
                    .list(&record.provider, &source_cwd, Some(&client))
                    .await
                    .map_err(|_| ErrorCode::AgentIo)?;
                if service.revision() != store_revision {
                    return Err(ErrorCode::CatalogBusy);
                }
                let preset = entries
                    .into_iter()
                    .find(|entry| entry.preset.name == name && source_name(entry.source) == source)
                    .ok_or(ErrorCode::InvalidMessage)?
                    .preset;
                let reference = ModelPresetReference {
                    name,
                    source,
                    cwd: source_cwd,
                    agent_cwd: record.cwd.clone(),
                };
                let current = record.config.clone().unwrap_or_default();
                let (config, state) = select(
                    &current,
                    record.model_preset.as_ref(),
                    reference.clone(),
                    &preset,
                );
                let (effective, _) =
                    select(&effective, record.model_preset.as_ref(), reference, &preset);
                client
                    .validate_selection(&provider_core::ports::agent_session::AgentSessionSpec {
                        provider: record.provider.clone(),
                        cwd: record.cwd.clone(),
                        config: effective,
                        confinement: None,
                    })
                    .await
                    .map_err(|_| ErrorCode::InvalidMessage)?;
                Ok(PreparedPreset {
                    id: record.id,
                    expected_revision: record.config_revision,
                    expected_created_at: record.created_at,
                    expected_provider: record.provider,
                    expected_cwd: record.cwd,
                    store_revision,
                    config,
                    state,
                })
            });
            let result = tokio::select! {
                () = sender.closed() => return,
                result = work => result.unwrap_or(Err(ErrorCode::CatalogBusy)),
            };
            let _ = sender.send(result);
            #[cfg(test)]
            notification.signal_lookup_finished();
        });
        Ok(receiver)
    }

    /// Commit a validated library snapshot only if no other configuration intervened.
    pub(crate) fn commit_preset(&self, prepared: &PreparedPreset) -> Result<(), model::ErrorCode> {
        use model::ErrorCode;
        let _guard = self
            .presets
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?
            .fence()
            .try_read_owned()
            .map_err(|_| ErrorCode::CatalogBusy)?;
        if self
            .presets
            .as_ref()
            .is_none_or(|service| service.revision() != prepared.store_revision)
        {
            return Err(ErrorCode::CatalogBusy);
        }
        if let Some(agent) = self.live.get(&prepared.id) {
            let mut effective = prepared.config.clone();
            if let Some(binding) = &agent.mcp {
                binding.apply(&mut effective);
            }
            agent
                .session
                .validate_config_update(&effective)
                .map_err(|_| ErrorCode::InvalidMessage)?;
        }
        let applied = std::sync::atomic::AtomicBool::new(false);
        self.registry
            .update(&prepared.id, &|current| {
                if current.created_at != prepared.expected_created_at
                    || current.config_revision != prepared.expected_revision
                    || current.provider != prepared.expected_provider
                    || current.cwd != prepared.expected_cwd
                    || current.archived_at.is_some()
                    || current.observation.is_some()
                {
                    return current.clone();
                }
                applied.store(true, std::sync::atomic::Ordering::Relaxed);
                let mut next = current.clone();
                next.config = Some(prepared.config.clone());
                next.model_preset = Some(prepared.state.clone());
                next.config_revision += 1;
                next.updated_at = crate::support::time::now_timestamp();
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        if !applied.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(ErrorCode::CatalogBusy);
        }
        Ok(())
    }
}

/// One bounded, read-only refresh owned by the Agent's session lane.
pub(super) struct Refresh {
    receiver: tokio::sync::oneshot::Receiver<Result<PreparedPreset, model::ErrorCode>>,
    started: std::time::Instant,
}

impl std::fmt::Debug for Refresh {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Refresh").finish_non_exhaustive()
    }
}

impl super::AgentManager {
    pub(crate) fn discard_preset_refresh(&mut self, id: &str) {
        self.preset_refreshes.remove(id);
        self.preset_ready.remove(id);
    }

    /// Ensure the next submitted turn freezes a freshly read preset snapshot, without awaiting
    /// the external preset store on the session lane. A pending lookup is polled by that lane.
    pub(super) fn ensure_preset_ready(&mut self, id: &str) -> Result<bool, model::ErrorCode> {
        use model::ErrorCode;
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        let Some(reference) = record
            .model_preset
            .as_ref()
            .and_then(|state| state.selected.as_ref())
        else {
            self.preset_refreshes.remove(id);
            self.preset_ready.remove(id);
            return Ok(true);
        };
        if reference.agent_cwd != record.cwd {
            return Err(ErrorCode::InvalidMessage);
        }
        let version = self
            .presets
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?
            .revision();
        if self.preset_ready.get(id) == Some(&(record.config_revision, version)) {
            return Ok(true);
        }
        if let Some(refresh) = self.preset_refreshes.get_mut(id) {
            let result = match refresh.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    Some(Err(ErrorCode::AgentIo))
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                    if refresh.started.elapsed() >= std::time::Duration::from_secs(60) =>
                {
                    Some(Err(ErrorCode::AgentIo))
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
            };
            let Some(result) = result else {
                return Ok(false);
            };
            self.preset_refreshes.remove(id);
            let prepared = match result {
                Err(ErrorCode::CatalogBusy) => return Ok(false),
                other => other?,
            };
            match self.commit_preset(&prepared) {
                Err(ErrorCode::CatalogBusy) => return Ok(false),
                other => other?,
            }
            let committed = self
                .registry
                .get(id)
                .map_err(|_| ErrorCode::AgentIo)?
                .ok_or(ErrorCode::AgentNotFound)?;
            self.preset_ready
                .insert(id.to_owned(), (committed.config_revision, version));
            return Ok(true);
        }
        let receiver =
            self.prepare_preset(id, &reference.name, &reference.source, Some(reference))?;
        self.preset_refreshes.insert(
            id.to_owned(),
            Refresh {
                receiver,
                started: std::time::Instant::now(),
            },
        );
        Ok(false)
    }
}

impl super::AgentManager {
    /// Resolve a draft preset against its current library before starting the new session.
    pub(crate) async fn prepare_creation_preset(
        &self,
        session: &crate::protocol::agent_execution::SessionConfig,
        source_cwd: &str,
        request: &crate::protocol::agent_execution::CreateModelPreset,
    ) -> Result<(StoredAgentConfig, ModelPresetState), model::ErrorCode> {
        use model::ErrorCode;
        if !domain::preset::valid_text(&request.name)
            || !matches!(request.source.as_str(), "ait" | "global" | "project")
            || request.base_feature_values.len() > 128
            || request.base_feature_values.keys().any(|key| !is_role(key))
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let client = self
            .clients
            .get(&session.provider)
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let service = self
            .presets
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        #[cfg(test)]
        service.signal_lookup_started();
        let _guard = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            service.fence().read_owned(),
        )
        .await
        .map_err(|_| ErrorCode::CatalogBusy)?;
        let version = service.revision();
        let preset = service
            .list(&session.provider, source_cwd, Some(client))
            .await
            .map_err(|_| ErrorCode::AgentIo)?
            .into_iter()
            .find(|entry| {
                entry.preset.name == request.name && source_name(entry.source) == request.source
            })
            .ok_or(ErrorCode::InvalidMessage)?
            .preset;
        if service.revision() != version {
            return Err(ErrorCode::CatalogBusy);
        }
        let mut base = session.stored.clone();
        let features = base.feature_values.get_or_insert_with(BTreeMap::new);
        features.retain(|field, _| !is_role(field));
        features.extend(request.base_feature_values.clone());
        if features.is_empty() {
            base.feature_values = None;
        }
        base.thinking_option_id
            .clone_from(&request.base_thinking_option_id);
        let reference = ModelPresetReference {
            name: request.name.clone(),
            source: request.source.clone(),
            cwd: source_cwd.to_owned(),
            agent_cwd: session.cwd.clone(),
        };
        Ok(select(&base, None, reference, &preset))
    }
}

impl super::AgentManager {
    /// Read the selected Agent's own preset scope; paths and internal baselines never leave host.
    pub(crate) async fn model_preset_context(&self, id: &str) -> Result<Value, model::ErrorCode> {
        use model::ErrorCode;
        let record = self
            .registry
            .get(id)
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        if record.observation.is_some() || record.internal {
            return Err(ErrorCode::UnsupportedCapability);
        }
        let selected = record
            .model_preset
            .as_ref()
            .and_then(|state| state.selected.as_ref());
        if selected.is_some_and(|reference| reference.agent_cwd != record.cwd) {
            return Ok(
                serde_json::json!({"agentId":id,"provider":record.provider,"presets":[],"selectedAvailable":false,"selectedModelPreset":selected.map(|reference| serde_json::json!({"name":reference.name,"source":reference.source})),"error":"Preset source is unavailable"}),
            );
        }
        let cwd = selected.map_or(record.cwd.as_str(), |reference| reference.cwd.as_str());
        let service = self
            .presets
            .as_ref()
            .ok_or(ErrorCode::UnsupportedCapability)?;
        let client = self
            .clients
            .get(&record.provider)
            .ok_or(ErrorCode::UnsupportedCapability)?;
        #[cfg(test)]
        service.signal_lookup_started();
        let _guard = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            service.fence().read_owned(),
        )
        .await
        .map_err(|_| ErrorCode::CatalogBusy)?;
        let version = service.revision();
        let entries = service.list(&record.provider, cwd, Some(client)).await;
        let current = self.registry.get(id).map_err(|_| ErrorCode::AgentIo)?;
        if service.revision() != version
            || current.as_ref().is_none_or(|current| {
                current.created_at != record.created_at
                    || current.config_revision != record.config_revision
                    || current.provider != record.provider
                    || current.cwd != record.cwd
            })
        {
            return Err(ErrorCode::CatalogBusy);
        }
        let selected_identity = selected
            .map(|reference| serde_json::json!({"name":reference.name,"source":reference.source}));
        match entries {
            Ok(presets) => {
                let available = selected.is_none_or(|reference| {
                    presets.iter().any(|entry| {
                        entry.preset.name == reference.name
                            && source_name(entry.source) == reference.source
                    })
                });
                Ok(
                    serde_json::json!({"agentId":id,"provider":record.provider,"presets":presets,"selectedAvailable":available,"selectedModelPreset":selected_identity,"error":null}),
                )
            }
            Err(_) => Ok(
                serde_json::json!({"agentId":id,"provider":record.provider,"presets":[],"selectedAvailable":false,"selectedModelPreset":selected.map(|reference| serde_json::json!({"name":reference.name,"source":reference.source})),"error":"Preset source is unavailable"}),
            ),
        }
    }
}
