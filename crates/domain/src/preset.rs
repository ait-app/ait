//! Persisted model presets owned by Ait for providers without a native preset store.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Largest accepted preset name or thinking identifier.
pub const MAX_PRESET_TEXT: usize = 256;
/// Largest model route, including a JSON-encoded native provider/model pair.
const MAX_PRESET_MODEL: usize = 16 * 1024;
/// Largest accepted number of presets per provider.
pub const MAX_PRESETS: usize = 64;
/// Largest accepted serialized feature map per preset.
pub const MAX_PRESET_BYTES: usize = 64 * 1024;
/// Largest accepted number of feature values in one preset.
const MAX_PRESET_FEATURES: usize = 128;
/// Largest accepted fallback chain length per role or target.
const MAX_FALLBACK_CHAIN: usize = 32;
/// Largest accepted fallback selector length.
const MAX_FALLBACK_SELECTOR: usize = 2048;

/// One named model selection, shaped like the client bundle so applying it is a copy.
///
/// Native adapters project their own definitions into this value. Adapters without a native
/// store persist it directly in daemon configuration.
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
    /// Provider feature values, including `modelRole.*` and `modelFallback.*` entries.
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

/// Whether `key` is a bounded feature name: a role/fallback target or a feature identifier.
fn valid_feature_key(key: &str) -> bool {
    if let Some(target) = key
        .strip_prefix("modelRole.")
        .or_else(|| key.strip_prefix("modelFallback."))
    {
        return !target.is_empty() && target.len() <= 128 && !target.chars().any(char::is_control);
    }
    let mut characters = key.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && key.len() <= 65
        && characters.all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-'))
}

/// Whether a `modelFallback.*` value is `"native"` or a bounded array of selector strings.
fn valid_fallback_value(value: &Value) -> bool {
    if value.as_str() == Some("native") {
        return true;
    }
    let Some(items) = value.as_array() else {
        return false;
    };
    items.len() <= MAX_FALLBACK_CHAIN
        && items.iter().all(|item| {
            item.as_str().is_some_and(|selector| {
                !selector.trim().is_empty()
                    && selector.len() <= MAX_FALLBACK_SELECTOR
                    && !selector.chars().any(char::is_control)
            })
        })
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
        if self.model.trim().is_empty()
            || self.model.len() > MAX_PRESET_MODEL
            || self.model.chars().any(char::is_control)
        {
            return Err("Preset model must be bounded text without control bytes".to_owned());
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
            if key.starts_with("modelFallback.") && !valid_fallback_value(value) {
                return Err(format!(
                    "Preset fallback chain {key:?} must be \"native\" or up to 32 model selectors"
                ));
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

#[cfg(test)]
mod tests;
