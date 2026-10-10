//! Paseo timeline queries and append-only display entries, separate from domain Messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable position in one timeline generation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    /// Opaque durable generation identity.
    pub(crate) epoch: String,
    /// Sequence position; committed rows start at one.
    pub(crate) seq: u64,
}

/// Page selection relative to a cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Direction {
    /// Most recent matching rows.
    Tail,
    /// Rows strictly before the cursor.
    Before,
    /// Rows strictly after the cursor.
    After,
}

/// Legacy requested view; the current Paseo API always returns the display projection.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Projection {
    /// Display projection without destructive rewriting of stored items.
    #[default]
    Projected,
    /// Accepted legacy spelling; responses still use the display projection.
    Canonical,
}

/// Bounded timeline page query.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FetchRequest {
    /// Registered Agent identifier.
    pub(crate) agent_id: String,
    /// Defaults to after with a cursor, otherwise tail.
    pub(crate) direction: Option<Direction>,
    /// Optional exclusive boundary.
    pub(crate) cursor: Option<Cursor>,
    /// Zero requests the entire window, subject to transport budgets.
    pub(crate) limit: Option<usize>,
    /// Legacy requested view, retained for request compatibility.
    #[serde(default)]
    #[expect(
        dead_code,
        reason = "validated for compatibility; responses always use the display view"
    )]
    pub(crate) projection: Projection,
    /// Echoed merge hint for a client loading discontiguous windows.
    pub(crate) merge_window: Option<bool>,
}

/// Case-insensitive text search over user and assistant display messages.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SearchRequest {
    /// Registered Agent identifier.
    pub(crate) agent_id: String,
    /// Nonempty search text.
    pub(crate) query: String,
    /// Exclusive sequence boundary, defaulting to zero.
    pub(crate) cursor: Option<usize>,
}

/// One immutable provider projection item before sequencing.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeItem {
    /// Provider-owned stable projection identity, scoped to its Agent.
    pub(crate) key: String,
    /// Native turn identity, when available.
    pub(crate) turn_id: Option<String>,
    /// RFC3339 source timestamp.
    pub(crate) timestamp: String,
    /// Paseo timeline item; never a host domain Message.
    pub(crate) item: Value,
}

/// Plugin display append request; plugin identity is supplied by connection provenance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppendRequest {
    /// Registered Agent identifier.
    #[expect(
        dead_code,
        reason = "routing resolves agentId before decoding the full request"
    )]
    pub(crate) agent_id: String,
    /// Display extension item, never provider prompt history.
    pub(crate) item: PluginItem,
}

/// Immutable plugin extension payload.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginItem {
    /// Must be `plugin`.
    pub(crate) r#type: String,
    /// Stable plugin-local identity used for idempotent append.
    pub(crate) id: String,
    /// Plugin-specific display kind.
    pub(crate) kind: String,
    /// Positive schema version.
    pub(crate) version: u32,
    /// JSON display data, capped at 64 KiB.
    pub(crate) data: Value,
}

/// Agent IDs selected by one independently releasable subscription.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SubscriptionRequest {
    /// At most 32 full IDs or unambiguous identifiers.
    pub(crate) agent_ids: Vec<String>,
}
