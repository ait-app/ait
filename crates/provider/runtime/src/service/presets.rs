//! One preset editor over two stores: a harness-owned one, or Ait's daemon configuration.

pub(crate) mod ait;

use std::sync::Arc;

use model::storage::daemon_config::DaemonConfigStore;

use provider_core::ports::agent_session::AgentClient;
use provider_core::ports::preset::{
    Preset, PresetBackend, PresetDescriptor, PresetEntry, PresetError, PresetSource, editable,
};

/// Largest accepted provider identifier on a preset request.
const MAX_PROVIDER: usize = 128;

/// Unified preset access for one provider, routed to whichever store owns it.
#[derive(Debug, Clone)]
pub struct PresetService {
    config: Arc<dyn DaemonConfigStore>,
    /// Serializes Ait-store mutations.
    ///
    /// Saving or deleting a preset reads the provider's whole list and writes it back, and the
    /// store's `patch` replaces that array wholesale. Two unserialized writes would each compute
    /// their list from the same snapshot, so the second would drop the first's entry while both
    /// still reported success. Preset requests run on the parallel discovery lane, so the whole
    /// read-modify-write is fenced here instead. One `PresetService` is composed per daemon and
    /// cloned into its consumers, which is what lets this lock span all of them.
    writes: Arc<std::sync::Mutex<()>>,
}

impl PresetService {
    /// Serve Ait-managed presets from `config`.
    pub fn new(config: Arc<dyn DaemonConfigStore>) -> Self {
        Self {
            config,
            writes: Arc::new(std::sync::Mutex::new(())),
        }
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
    ) -> Result<Vec<PresetEntry>, PresetError> {
        let descriptor = self.describe(cwd, client).await;
        if descriptor.backend == PresetBackend::Native {
            let client = client.ok_or_else(|| {
                PresetError::Unavailable("This provider is not installed on the host".to_owned())
            })?;
            // A native store reports its private overlay; the editor may only see what a user
            // can actually edit.
            return client.native_presets(cwd).await.map(|presets| {
                presets
                    .into_iter()
                    .map(|mut entry| {
                        entry.preset = editable(entry.preset);
                        entry
                    })
                    .collect()
            });
        }
        self.ait()
            .list(provider)
            .map(|presets| {
                presets
                    .into_iter()
                    .map(|preset| PresetEntry {
                        preset,
                        source: PresetSource::Ait,
                        editable: true,
                    })
                    .collect()
            })
            .map_err(PresetError::Unavailable)
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
        original: Option<&str>,
    ) -> Result<(), PresetError> {
        let preset = &editable(preset.clone());
        preset.validate().map_err(PresetError::Rejected)?;
        let descriptor = self.describe(cwd, client).await;
        if descriptor.backend == PresetBackend::Native {
            if original.is_some_and(|original| original != preset.name) {
                return Err(PresetError::Rejected(
                    "This native preset store does not support rename".to_owned(),
                ));
            }
            if !descriptor.can_edit && !descriptor.can_create {
                return Err(PresetError::Unavailable(reason(&descriptor)));
            }
            let client = client.ok_or_else(|| {
                PresetError::Unavailable("This provider is not installed on the host".to_owned())
            })?;
            return client.save_native_preset(cwd, preset).await;
        }
        // The lock spans the read-modify-write, not just the write: two concurrent saves of
        // different presets would otherwise both list the same store and the later one would
        // publish a list that drops the other's entry, each still reporting success.
        let _guard = self
            .writes
            .lock()
            .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))?;
        self.ait()
            .save(provider, preset, original)
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
        if !domain::preset::valid_text(name) {
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
        // Deleting is the same list-wide rewrite as saving, so it takes the same fence.
        let _guard = self
            .writes
            .lock()
            .map_err(|_| PresetError::Unavailable("Preset store is unavailable".to_owned()))?;
        self.ait()
            .delete(provider, name)
            .map_err(PresetError::Rejected)
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
