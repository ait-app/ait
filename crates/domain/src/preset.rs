//! Persisted model presets owned by Ait for providers without a native preset store.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Largest accepted preset name or model identifier.
pub const MAX_PRESET_TEXT: usize = 256;
/// Largest accepted number of presets per provider.
pub const MAX_PRESETS: usize = 64;
/// Largest accepted serialized feature map per preset.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;
/// Largest accepted number of feature values in one preset.
const MAX_PRESET_FEATURES: usize = 64;

/// One named model selection, shaped like the client bundle so applying it is a copy.
///
/// A provider with its own preset store (OMP) never uses this record; it exists for the
/// adapters whose harness has no such concept, and is persisted in daemon configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    /// Ait-assigned preset name.
    pub name: String,
    /// Required main model.
    pub model: String,
    /// Optional thinking or effort option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_option_id: Option<String>,
    /// Provider feature values, including `modelRole.*` entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub feature_values: BTreeMap<String, Value>,
}

/// Whether `value` is bounded, control-free and not blank.
#[must_use]
pub fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_PRESET_TEXT
        && !value.chars().any(char::is_control)
}

/// Whether `key` is a bounded feature name: a letter followed by up to 64 name bytes.
fn valid_feature_key(key: &str) -> bool {
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && key.len() <= 65
        && characters.all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
}

impl ProviderPreset {
    /// Reject an unusable preset before it reaches a store or a client.
    ///
    /// # Errors
    /// Returns the first structural violation as a client-readable reason.
    pub fn validate(&self) -> Result<(), String> {
        if !valid_text(&self.name) {
            return Err("Preset name must be 1-256 characters without control bytes".to_owned());
        }
        if !valid_text(&self.model) {
            return Err("Preset model must be 1-256 characters without control bytes".to_owned());
        }
        if let Some(thinking) = &self.thinking_option_id
            && !valid_text(thinking)
        {
            return Err("Preset thinking option must be bounded text".to_owned());
        }
        if self.feature_values.len() > MAX_PRESET_FEATURES {
            return Err("Preset has too many feature values".to_owned());
        }
        for (key, value) in &self.feature_values {
            if !valid_feature_key(key) {
                return Err(format!("Preset feature key {key:?} is not a valid name"));
            }
            if value.is_null() {
                return Err(format!("Preset feature {key:?} must not be null"));
            }
        }
        let bytes = serde_json::to_vec(&self.feature_values)
            .map_err(|_| "Preset feature values are not serializable".to_owned())?;
        if bytes.len() > MAX_PRESET_BYTES {
            return Err("Preset feature values exceed the configuration budget".to_owned());
        }
        Ok(())
    }
}

/// Validate a whole provider preset list, including its size bound and name uniqueness.
///
/// # Errors
/// Returns the first structural violation; an empty list is always valid.
pub fn validate_presets(presets: &[ProviderPreset]) -> Result<(), String> {
    if presets.len() > MAX_PRESETS {
        return Err("Too many presets for one provider".to_owned());
    }
    let mut names = BTreeSet::new();
    for preset in presets {
        preset.validate()?;
        if !names.insert(preset.name.as_str()) {
            return Err(format!("Preset {:?} is defined twice", preset.name));
        }
    }
    Ok(())
}

/// Whether `presets` is a valid stored preset list.
#[must_use]
pub fn presets_are_valid(presets: &[ProviderPreset]) -> bool {
    validate_presets(presets).is_ok()
}
