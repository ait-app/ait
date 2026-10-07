//! Metadata-owned server methods and ping payload.

use serde::{Deserialize, Serialize};

/// Application ping parameters; extra fields remain accepted for compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    /// Bounded correlation nonce echoed by the server.
    pub nonce: String,
}

/// Canonical client activity heartbeat event owned by metadata.
pub const HEARTBEAT_METHOD: &str = "session.heartbeat";
