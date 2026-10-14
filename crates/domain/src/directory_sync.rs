//! Directory checkpoints, change metadata and sequenced read values.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Optional checkpoint for a complete directory read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    /// Generation returned by the preceding read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    /// Last observed sequence in this collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<u64>,
}

/// Snapshot or compacted latest-state changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Process generation; changes after a restart require a new snapshot.
    pub generation: String,
    /// Latest assigned sequence, including removals.
    pub head_seq: u64,
    /// Whether the response replaces or patches the client collection.
    pub mode: Mode,
    /// Why the server replaced the collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// Deleted identities in ascending sequence order.
    pub removals: Vec<Removal>,
}

/// How a directory response applies to a client cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Replace the cache with the full response.
    Snapshot,
    /// Apply only returned upserts and removals.
    Changes,
}

/// Reason for falling back to a full directory snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// No complete checkpoint was supplied.
    NoCursor,
    /// The server generation no longer matches.
    GenerationChanged,
    /// The checkpoint is ahead of the server or its removals have expired.
    CursorExpired,
}

/// One retained deletion marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Removal {
    /// Deleted entity identity.
    pub id: String,
    /// Sequence assigned to the deletion.
    pub seq: u64,
}

/// Sequenced rows and their collection checkpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// Complete rows annotated with `syncSeq`.
    pub values: Vec<Value>,
    /// Checkpoint and deleted identities.
    pub sync: Metadata,
}
