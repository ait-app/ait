//! Ait-managed presets for providers whose harness has no native preset store.
//!
//! The records live in daemon configuration under `providers.<id>.modelPresets`, so one provider
//! can be rewritten without touching another and a daemon restart preserves the list.

use std::sync::Arc;

use domain::preset::validate_presets;
use model::storage::daemon_config::DaemonConfigStore;
use serde_json::json;

use provider_core::ports::preset::Preset;

/// The daemon-config key that holds one provider's managed presets.
pub(crate) const FIELD: &str = "modelPresets";

/// Failure of an Ait-managed preset operation; every message is client-readable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AitPresetError {
    /// Daemon configuration could not be read.
    #[error("Daemon configuration could not be read")]
    ConfigUnreadable,
    /// The stored list does not decode as presets.
    #[error("Stored presets are malformed")]
    Malformed,
    /// A preset or list broke a domain rule; carries the domain's client-readable reason.
    #[error("{0}")]
    Invalid(String),
    /// The validated list could not be encoded for storage.
    #[error("Presets could not be serialized")]
    Unserializable,
    /// Daemon configuration could not be written.
    #[error("Daemon configuration could not be written")]
    ConfigUnwritable,
    /// The named preset does not exist.
    #[error("Preset {0:?} is not defined")]
    NotDefined(String),
    /// A rename target already belongs to another preset.
    #[error("Preset {0:?} already exists")]
    AlreadyExists(String),
}

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
    fn read(&self, provider: &str) -> Result<Vec<Preset>, AitPresetError> {
        let config = self
            .config
            .get()
            .map_err(|_| AitPresetError::ConfigUnreadable)?;
        let stored = config
            .get("providers")
            .and_then(|providers| providers.get(provider))
            .and_then(|entry| entry.get(FIELD));
        let Some(stored) = stored.filter(|stored| !stored.is_null()) else {
            return Ok(Vec::new());
        };
        let presets: Vec<Preset> =
            serde_json::from_value(stored.clone()).map_err(|_| AitPresetError::Malformed)?;
        validate_presets(&presets).map_err(AitPresetError::Invalid)?;
        Ok(presets)
    }

    /// Persist one provider's whole preset list, leaving every other provider untouched.
    fn write(&self, provider: &str, presets: &[Preset]) -> Result<(), AitPresetError> {
        validate_presets(presets).map_err(AitPresetError::Invalid)?;
        let value = serde_json::to_value(presets).map_err(|_| AitPresetError::Unserializable)?;
        self.config
            .patch(&json!({"providers":{provider:{FIELD:value}}}))
            .map_err(|_| AitPresetError::ConfigUnwritable)?;
        Ok(())
    }
}

impl AitPresets {
    /// Read one provider's stored presets.
    ///
    /// # Errors
    /// Returns a client-readable reason when storage is unreadable or malformed.
    pub(crate) fn list(&self, provider: &str) -> Result<Vec<Preset>, AitPresetError> {
        self.read(provider)
    }

    /// Create or replace one preset, leaving other providers untouched.
    ///
    /// `original` names the preset being edited. When it differs from `preset.name` this is a
    /// rename: the old entry is replaced in the same write, and a different preset already
    /// using the new name rejects instead of being silently overwritten.
    ///
    /// # Errors
    /// Returns a client-readable rejection before writing anything.
    pub(crate) fn save(
        &self,
        provider: &str,
        preset: &Preset,
        original: Option<&str>,
    ) -> Result<(), AitPresetError> {
        preset.validate().map_err(AitPresetError::Invalid)?;
        let mut presets = self.read(provider)?;
        let existing = presets.iter().position(|stored| stored.name == preset.name);
        let index = match original {
            Some(original) if original != preset.name => {
                // The record being edited must exist; the new name must not belong to another.
                let source = presets
                    .iter()
                    .position(|stored| stored.name == original)
                    .ok_or_else(|| AitPresetError::NotDefined(original.to_owned()))?;
                if existing.is_some_and(|index| index != source) {
                    return Err(AitPresetError::AlreadyExists(preset.name.clone()));
                }
                source
            }
            _ => {
                let Some(index) = existing else {
                    presets.push(preset.clone());
                    return self.write(provider, &presets);
                };
                index
            }
        };
        presets[index] = preset.clone();
        self.write(provider, &presets)
    }

    /// Remove one preset by name.
    ///
    /// # Errors
    /// Returns a client-readable rejection before writing anything.
    pub(crate) fn delete(&self, provider: &str, name: &str) -> Result<(), AitPresetError> {
        let mut presets = self.read(provider)?;
        let before = presets.len();
        presets.retain(|preset| preset.name != name);
        if presets.len() == before {
            return Err(AitPresetError::NotDefined(name.to_owned()));
        }
        self.write(provider, &presets)
    }
}
