//! Append-only display entries that native adapters project, separate from domain Messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One immutable provider projection item before sequencing.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeItem {
    /// Provider-owned stable projection identity, scoped to its Agent.
    pub key: String,
    /// Native turn identity, when available.
    pub turn_id: Option<String>,
    /// RFC3339 source timestamp.
    pub timestamp: String,
    /// Paseo timeline item; never a host domain Message.
    pub item: Value,
}
