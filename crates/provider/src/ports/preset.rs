//! Provider-owned model presets behind one backend-agnostic descriptor.
//!
//! Clients render a single preset editor for every provider. Only the store behind it differs:
//! a harness with its own presets (OMP) answers from that store, and every other adapter is
//! served from daemon configuration. The editor never branches on the provider identity.

use domain::preset::ProviderPreset;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) use domain::preset::valid_text;

/// Where a provider's presets actually live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum PresetBackend {
    /// The native harness owns the presets and exposes its own save/delete commands.
    Native,
    /// Ait persists the presets in daemon configuration for this provider.
    Ait,
}

/// What the preset store can do, so the editor disables exactly the missing operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PresetDescriptor {
    /// Store that answers `list`, `save` and `delete`.
    pub(crate) backend: PresetBackend,
    /// Whether a new preset can be created.
    pub(crate) can_create: bool,
    /// Whether an existing preset can be replaced in place.
    pub(crate) can_edit: bool,
    /// Whether a preset can be removed.
    pub(crate) can_delete: bool,
    /// Whether a preset can be renamed without deleting and recreating it.
    pub(crate) can_rename: bool,
    /// Why the store is unavailable or restricted; shown instead of a dead control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
}

impl PresetDescriptor {
    /// Every operation supported by one fully manageable store.
    pub(crate) fn managed(backend: PresetBackend) -> Self {
        Self {
            backend,
            can_create: true,
            can_edit: true,
            can_delete: true,
            can_rename: true,
            reason: None,
        }
    }

    /// A store that cannot do anything, with the reason a client should display.
    pub(crate) fn unavailable(backend: PresetBackend, reason: &str) -> Self {
        Self {
            backend,
            can_create: false,
            can_edit: false,
            can_delete: false,
            can_rename: false,
            reason: Some(reason.to_owned()),
        }
    }
}

/// One preset as the store returns it.
pub(crate) type Preset = ProviderPreset;

/// A preset failure carried to the client as a readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PresetError {
    /// The request violates a structural rule; nothing was written.
    Rejected(String),
    /// The store cannot answer now; nothing was written.
    Unavailable(String),
}

impl PresetError {
    /// Client-readable reason.
    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Rejected(reason) | Self::Unavailable(reason) => reason,
        }
    }
}

/// Render stored presets into the `modelPreset` select every client already understands.
///
/// `native` is the provider's own preset feature, when it has one. A provider with a native
/// store keeps it untouched and never receives Ait presets on top, so the two stores can never
/// disagree; only a provider without native presets and with stored ones is synthesized.
pub(crate) fn synthesize(
    native: Option<Value>,
    stored: &[Preset],
    current_model: Option<&str>,
    current_thinking: Option<&str>,
    current_features: Option<&std::collections::BTreeMap<String, Value>>,
) -> Option<Value> {
    if native.is_some() {
        return native;
    }
    if stored.is_empty() {
        return None;
    }
    let mut options = vec![serde_json::json!({"id":"__custom__","label":"Custom configuration"})];
    let mut selected = "__custom__";
    for preset in stored {
        let mut configuration = serde_json::Map::new();
        configuration.insert("modelId".to_owned(), Value::String(preset.model.clone()));
        if let Some(thinking) = &preset.thinking_option_id {
            configuration.insert(
                "thinkingOptionId".to_owned(),
                Value::String(thinking.clone()),
            );
        }
        if !preset.feature_values.is_empty() {
            configuration.insert(
                "featureValues".to_owned(),
                Value::Object(preset.feature_values.clone().into_iter().collect()),
            );
        }
        if selected == "__custom__"
            && matches_configuration(preset, current_model, current_thinking, current_features)
        {
            selected = preset.name.as_str();
        }
        options.push(serde_json::json!({"id":preset.name,"label":preset.name,
            "metadata":{"source":"ait","configuration":Value::Object(configuration)}}));
    }
    Some(serde_json::json!({"type":"select","id":"modelPreset","label":"Model preset",
        "value":selected,"options":options}))
}

/// Whether a stored preset describes exactly the current selection.
///
/// Mirrors the client's `withModelPresetSelection`: model and thinking must match, and every
/// stored feature must already be in effect. Preset bookkeeping keys are ignored.
fn matches_configuration(
    preset: &Preset,
    model: Option<&str>,
    thinking: Option<&str>,
    features: Option<&std::collections::BTreeMap<String, Value>>,
) -> bool {
    model == Some(preset.model.as_str())
        && thinking == preset.thinking_option_id.as_deref()
        && preset.feature_values.iter().all(|(key, value)| {
            key == "modelPreset"
                || key == "modelPresetThinking"
                || key == "modelPresetRoles"
                || features.and_then(|features| features.get(key)) == Some(value)
        })
}

#[cfg(test)]
mod tests;
