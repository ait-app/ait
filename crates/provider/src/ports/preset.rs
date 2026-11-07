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
///
/// The four flat booleans are the wire contract the editor reads; an operation set would only
/// move the same four facts behind a translation layer.
#[allow(clippy::struct_excessive_bools)]
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
    ///
    /// A rename is a single write that replaces one record under a new name. An Ait store does
    /// that atomically and rejects a name already in use, so the editor may offer it. A native
    /// harness only exposes save and delete, where a rename would be a non-atomic save-then-
    /// delete, so the descriptor reports it closed and carries the reason instead.
    pub(crate) fn managed(backend: PresetBackend) -> Self {
        Self {
            backend,
            can_create: true,
            can_edit: true,
            can_delete: true,
            can_rename: backend == PresetBackend::Ait,
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

/// Feature keys that describe a provider's own preset patch, never a user-editable selection.
///
/// OMP writes these into its private overlay (`modelPreset` is a null marker, the thinking and
/// roles keys mirror the native preset entry). They must not travel back to the editor as
/// editable feature values, and a preset built from them must not be validated against the
/// user-facing rule that forbids nulls.
const INTERNAL_FEATURE_KEYS: &[&str] = &["modelPreset", "modelPresetThinking", "modelPresetRoles"];

/// Whether `key` is provider bookkeeping rather than an editable feature value.
pub(crate) fn is_internal_feature(key: &str) -> bool {
    INTERNAL_FEATURE_KEYS.contains(&key)
}

/// Strip internal patch markers and null placeholders from a preset before it is stored.
///
/// A native store returns its private overlay as feature values; a client editing that record
/// echoes it back. Only the user-editable remainder is meaningful, so the conversion happens
/// once here instead of in every backend.
pub(crate) fn editable(mut preset: Preset) -> Preset {
    preset
        .feature_values
        .retain(|key, value| !is_internal_feature(key) && !value.is_null());
    preset
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
    Some(
        serde_json::json!({"type":"select","id":"modelPreset","label":"Model preset",
        "value":selected,"options":options}),
    )
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
