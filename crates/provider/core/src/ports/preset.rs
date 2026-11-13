//! Provider-owned model presets behind one backend-agnostic descriptor.
//!
//! Clients render a single preset editor for every provider. Only the store behind it differs:
//! a harness with its own presets (OMP) answers from that store, and every other adapter is
//! served from daemon configuration. The editor never branches on the provider identity.

use domain::preset::ProviderPreset;
use serde::{Deserialize, Serialize};

/// Where a provider's presets actually live.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetBackend {
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
pub struct PresetDescriptor {
    /// Store that answers `list`, `save` and `delete`.
    pub backend: PresetBackend,
    /// Whether a new preset can be created.
    pub can_create: bool,
    /// Whether an existing preset can be replaced in place.
    pub can_edit: bool,
    /// Whether a preset can be removed.
    pub can_delete: bool,
    /// Whether a preset can be renamed without deleting and recreating it.
    pub can_rename: bool,
    /// Why the store is unavailable or restricted; shown instead of a dead control.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl PresetDescriptor {
    /// Every operation supported by one fully manageable store.
    ///
    /// A rename is a single write that replaces one record under a new name. An Ait store does
    /// that atomically and rejects a name already in use, so the editor may offer it. A native
    /// harness only exposes save and delete, where a rename would be a non-atomic save-then-
    /// delete, so the descriptor reports it closed and carries the reason instead.
    #[must_use]
    pub fn managed(backend: PresetBackend) -> Self {
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
    #[must_use]
    pub fn unavailable(backend: PresetBackend, reason: &str) -> Self {
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
pub type Preset = ProviderPreset;

/// Owning layer of one listed preset, supplied by the same read as its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetSource {
    /// Stored in Ait's daemon configuration.
    Ait,
    /// Stored in the native global configuration.
    Global,
    /// A project definition overrides the global name and is read-only in this editor.
    Project,
}

/// Read projection for the editor; ownership metadata is never persisted as preset content.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PresetEntry {
    /// The editable configuration, flattened for backwards-compatible list responses.
    #[serde(flatten)]
    pub preset: Preset,
    /// Layer that owns this exact listed definition.
    pub source: PresetSource,
    /// Whether this definition can be changed through the owning store's editor.
    pub editable: bool,
}

/// A preset failure carried to the client as a readable reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresetError {
    /// The request violates a structural rule; nothing was written.
    #[error("{0}")]
    Rejected(String),
    /// The store cannot answer now; nothing was written.
    #[error("{0}")]
    Unavailable(String),
}

impl PresetError {
    /// Client-readable reason.
    #[must_use]
    pub fn message(&self) -> &str {
        match self {
            Self::Rejected(reason) | Self::Unavailable(reason) => reason,
        }
    }
}

/// Feature keys that describe a provider's own preset patch, never a user-editable selection.
///
/// Earlier unreleased implementations used these private patch markers. They must not travel
/// back to the editor or into saved records as user-editable role selections.
const INTERNAL_FEATURE_KEYS: &[&str] = &["modelPreset", "modelPresetThinking", "modelPresetRoles"];

/// Whether `key` is provider bookkeeping rather than an editable feature value.
#[must_use]
fn is_internal_feature(key: &str) -> bool {
    INTERNAL_FEATURE_KEYS.contains(&key)
}

/// Strip legacy internal patch markers from a preset before it is stored.
///
/// Editable values, including invalid nulls, remain intact so the domain validator can reject
/// them without silently dropping the user's configuration.
#[must_use]
pub fn editable(mut preset: Preset) -> Preset {
    preset
        .feature_values
        .retain(|key, _| !is_internal_feature(key));
    preset
}

#[cfg(test)]
mod tests;
