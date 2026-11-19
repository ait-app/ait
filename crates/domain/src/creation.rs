//! Creation receipts and admission values shared by resource creation workflows.

use serde::{Deserialize, Serialize};

use crate::creation::protocol::Snapshot;

/// Durable creation progress payloads.
pub mod protocol;

/// Persisted immutable creation intent and its latest committed progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Opaque, kind-qualified digest of the idempotency key.
    pub id: String,
    /// Version-one SHA-256 fingerprint of the request, excluding its key and subscription flag.
    pub intent_hash: [u8; 32],
    /// Latest committed progress and reserved identities.
    pub snapshot: Snapshot,
    /// A proven initial Agent startup failure may retry the reserved identity.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub retry_initial_agent: bool,
}

/// Admission result: only a newly accepted intent may perform resource side effects.
#[derive(Debug)]
pub struct Admission {
    /// Whether the caller owns the first attempt.
    pub execute: bool,
    /// Latest committed state, including IDs reserved before the side effect.
    pub snapshot: Snapshot,
}
