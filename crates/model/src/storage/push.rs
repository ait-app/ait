//! Storage contract for durable push token leases.

use std::fmt;

use domain::storage::push::PushError;
use serde_json::Value;

/// Storage boundary for a complete subscription snapshot.
pub trait TokenStore: Send + fmt::Debug {
    /// Load Paseo's current or legacy JSON document; absent storage returns an empty object.
    /// # Errors
    /// Returns a safe read or document error.
    fn load(&self) -> Result<Value, PushError>;
    /// Atomically persist the new document before changing in-memory state.
    /// # Errors
    /// Returns a safe persistence error; the previous document must remain intact.
    fn save(&self, document: &Value) -> Result<(), PushError>;
}
