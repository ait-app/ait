//! Ait-managed presets for providers whose harness has no native preset store.
//!
//! The records live in daemon configuration under `providers.<id>.modelPresets`, so one provider
//! can be rewritten without touching another and a daemon restart preserves the list.

use std::sync::Arc;

use domain::preset::validate_presets;
use model::storage::daemon_config::DaemonConfigStore;
use serde_json::json;

use crate::ports::preset::Preset;

/// The daemon-config key that holds one provider's managed presets.
pub(crate) const FIELD: &str = "modelPresets";

/// Preset store backed by daemon configuration.
#[derive(Debug, Clone)]
pub(crate) struct AitPresets {
    config: Arc<dyn DaemonConfigStore>,
}

impl AitPresets {
    /// Read and write one provider's presets through `config`.
    pub(crate) fn new(config: Arc<dyn DaemonConfigStore>) -> Self {
        Self { config }
    }

    /// The current preset list for `provider`, parsed strictly enough to reject broken storage.
    fn read(&self, provider: &str) -> Result<Vec<Preset>, String> {
        let config = self
            .config
            .get()
            .map_err(|_| "Daemon configuration could not be read".to_owned())?;
        let stored = config
            .get("providers")
            .and_then(|providers| providers.get(provider))
            .and_then(|entry| entry.get(FIELD));
        let Some(stored) = stored.filter(|stored| !stored.is_null()) else {
            return Ok(Vec::new());
        };
        let presets: Vec<Preset> = serde_json::from_value(stored.clone())
            .map_err(|_| "Stored presets are malformed".to_owned())?;
        validate_presets(&presets)?;
        Ok(presets)
    }

    /// Persist one provider's whole preset list, leaving every other provider untouched.
    fn write(&self, provider: &str, presets: &[Preset]) -> Result<(), String> {
        validate_presets(presets)?;
        let value = serde_json::to_value(presets)
            .map_err(|_| "Presets could not be serialized".to_owned())?;
        self.config
            .patch(&json!({"providers":{provider:{FIELD:value}}}))
            .map_err(|_| "Daemon configuration could not be written".to_owned())?;
        Ok(())
    }
}

impl AitPresets {
    /// Read one provider's stored presets.
    ///
    /// # Errors
    /// Returns a client-readable reason when storage is unreadable or malformed.
    pub(crate) fn list(&self, provider: &str) -> Result<Vec<Preset>, String> {
        self.read(provider)
    }

    /// Create or replace one preset, leaving other providers untouched.
    ///
    /// # Errors
    /// Returns a client-readable rejection before writing anything.
    pub(crate) fn save(&self, provider: &str, preset: &Preset) -> Result<(), String> {
        preset.validate()?;
        let mut presets = self.read(provider)?;
        match presets
            .iter_mut()
            .find(|existing| existing.name == preset.name)
        {
            Some(existing) => *existing = preset.clone(),
            None => presets.push(preset.clone()),
        }
        self.write(provider, &presets)
    }

    /// Remove one preset by name.
    ///
    /// # Errors
    /// Returns a client-readable rejection before writing anything.
    pub(crate) fn delete(&self, provider: &str, name: &str) -> Result<(), String> {
        let mut presets = self.read(provider)?;
        let before = presets.len();
        presets.retain(|preset| preset.name != name);
        if presets.len() == before {
            return Err(format!("Preset {name:?} is not defined"));
        }
        self.write(provider, &presets)
    }
}
