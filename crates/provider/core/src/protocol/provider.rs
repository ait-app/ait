//! Provider-owned model, mode and feature definitions.

use serde_json::Value;

/// Provider-owned model and mode definitions, with no credentials or private diagnostics.
#[derive(Debug, Clone, Default)]
pub struct Details {
    /// Paseo model definitions fetched from the native provider.
    pub models: Vec<Value>,
    /// Modes actually supported by this server adapter.
    pub modes: Vec<Value>,
    /// Runtime toggles and selects supported by the adapter.
    pub features: Vec<Value>,
}
