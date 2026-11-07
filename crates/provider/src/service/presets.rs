//! One preset editor over two stores: a harness-owned one, or Ait's daemon configuration.

pub(crate) mod ait;

use std::sync::Arc;

use model::storage::daemon_config::DaemonConfigStore;
use serde_json::Value;

use crate::ports::agent_session::AgentClient;
use crate::ports::preset::{Preset, PresetBackend, PresetDescriptor, PresetError, synthesize};

/// Largest accepted provider identifier on a preset request.
const MAX_PROVIDER: usize = 128;

/// Unified preset access for one provider, routed to whichever store owns it.
#[derive(Debug, Clone)]
pub struct PresetService {
    config: Arc<dyn DaemonConfigStore>,
}

impl PresetService {
    /// Serve Ait-managed presets from `config`.
    pub fn new(config: Arc<dyn DaemonConfigStore>) -> Self {
        Self { config }
    }

    fn ait(&self) -> ait::AitPresets {
        ait::AitPresets::new(self.config.clone())
    }

    /// Report the store and supported operations for `provider`.
    ///
    /// The descriptor comes from the adapter, so a native store that this harness version
    /// cannot drive is reported read-only instead of failing later on a dead button.
    pub(crate) async fn describe(
        &self,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
    ) -> PresetDescriptor {
        match client {
            Some(client) => client.preset_descriptor(cwd).await,
            None => PresetDescriptor::unavailable(
                PresetBackend::Ait,
                "This provider is not installed on the host",
            ),
        }
    }

    /// List the presets the editor must show for `provider`.
    ///
    /// A native store answers fully; it is never merged with Ait records, so a provider can
    /// never show two competing lists.
    pub(crate) async fn list(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
    ) -> Result<Vec<Preset>, PresetError> {
        let descriptor = self.describe(cwd, client).await;
        if descriptor.backend == PresetBackend::Native {
            let client = client.ok_or_else(|| {
                PresetError::Unavailable("This provider is not installed on the host".to_owned())
            })?;
            return client.native_presets(cwd).await;
        }
        self.ait().list(provider).map_err(PresetError::Unavailable)
    }

    /// Create or replace one preset through the store that owns it.
    ///
    /// # Errors
    /// Rejects an unusable preset before any store is touched.
    pub(crate) async fn save(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        preset: &Preset,
    ) -> Result<(), PresetError> {
        preset.validate().map_err(PresetError::Rejected)?;
        let descriptor = self.describe(cwd, client).await;
        if descriptor.backend == PresetBackend::Native {
            if !descriptor.can_edit && !descriptor.can_create {
                return Err(PresetError::Unavailable(reason(&descriptor)));
            }
            let client = client.ok_or_else(|| {
                PresetError::Unavailable("This provider is not installed on the host".to_owned())
            })?;
            return client.save_native_preset(cwd, preset).await;
        }
        self.ait()
            .save(provider, preset)
            .map_err(PresetError::Rejected)
    }

    /// Remove one preset through the store that owns it.
    ///
    /// # Errors
    /// Rejects an unknown name without writing anything.
    pub(crate) async fn delete(
        &self,
        provider: &str,
        cwd: &str,
        client: Option<&Arc<dyn AgentClient>>,
        name: &str,
    ) -> Result<(), PresetError> {
        if !crate::ports::preset::valid_text(name) {
            return Err(PresetError::Rejected(
                "Preset name must be 1-256 characters without control bytes".to_owned(),
            ));
        }
        let descriptor = self.describe(cwd, client).await;
        if descriptor.backend == PresetBackend::Native {
            if !descriptor.can_delete {
                return Err(PresetError::Unavailable(reason(&descriptor)));
            }
            let client = client.ok_or_else(|| {
                PresetError::Unavailable("This provider is not installed on the host".to_owned())
            })?;
            return client.delete_native_preset(cwd, name).await;
        }
        self.ait()
            .delete(provider, name)
            .map_err(PresetError::Rejected)
    }

    /// Merge the unified preset select into one provider's feature list.
    ///
    /// A native `modelPreset` already in `features` wins untouched, so OMP keeps its exact
    /// behavior. A provider that owns a native store never receives Ait entries on top, so the
    /// two stores can never disagree. Only an Ait-backed provider gains the synthesized select;
    /// with no stored presets the list is unchanged and the client keeps its empty state.
    ///
    /// `native_store` is the adapter's static capability, not the result of a probe, so this
    /// stays cheap enough for the draft/features path.
    pub(crate) fn apply(
        &self,
        provider: &str,
        native_store: bool,
        features: &mut Vec<Value>,
        current: &domain::agent_runtime::StoredAgentConfig,
    ) {
        // A feature the adapter already reports is left exactly where it is: the native store
        // owns that control, and moving it would reorder the caller's list for no gain.
        if features
            .iter()
            .any(|feature| feature["id"] == "modelPreset")
        {
            return;
        }
        if native_store {
            return;
        }
        let stored = self.ait().list(provider).unwrap_or_default();
        if let Some(feature) = synthesize(
            None,
            &stored,
            current.model.as_deref(),
            current.thinking_option_id.as_deref(),
            current.feature_values.as_ref(),
        ) {
            features.insert(0, feature);
        }
    }
}

fn reason(descriptor: &PresetDescriptor) -> String {
    descriptor
        .reason
        .clone()
        .unwrap_or_else(|| "This provider does not support that preset operation".to_owned())
}

/// Whether a request's provider identifier is usable.
pub(crate) fn valid_provider(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= MAX_PROVIDER
        && !provider.chars().any(char::is_control)
}

#[cfg(test)]
mod tests;
