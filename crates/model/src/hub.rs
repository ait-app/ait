//! Host publication ports. Authentication, transport and durable storage remain adapters.

use std::{fmt, future::Future, pin::Pin};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;
use uuid::Uuid;

/// Publication authority. A different center or node session must never receive old queued data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scope {
    /// Canonical authenticated center origin, without credentials.
    pub center: String,
    /// Session from the accepted serve grant.
    pub node_session_id: Uuid,
}

/// Discovered center capabilities, retained across temporary physical disconnections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    /// Current publication authority; absent after explicit stop or authorization rejection.
    pub scope: Option<Scope>,
    /// Whether calls can currently be sent.
    pub online: bool,
    /// Current logical link, used to republish snapshots after a fresh link.
    pub link: Option<Uuid>,
    /// Methods advertised by the latest welcome; empty for legacy centers.
    pub methods: Vec<String>,
}

impl LinkState {
    /// Whether the authenticated center advertises `method`, even while reconnecting.
    #[must_use]
    pub fn supports(&self, method: &str) -> bool {
        self.scope.is_some() && self.methods.iter().any(|available| available == method)
    }
}

/// Safe center call failures; never include request bodies or provider credentials.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// No matching online link, or the call lost its logical link before a reply.
    #[error("center link unavailable")]
    Unavailable,
    /// The center did not advertise this method.
    #[error("center method unsupported")]
    Unsupported,
    /// Bounded call admission is full.
    #[error("center call capacity reached")]
    Capacity,
    /// An invalid reply or oversized request was encountered.
    #[error("invalid center call or reply")]
    Protocol,
    /// A business error code, without the center's free-form error message.
    #[error("center rejected publication: {0}")]
    Rejected(String),
}

/// One call completes only after a business reply, never on a transport acknowledgement.
pub type CallFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, CallError>> + Send + 'a>>;

/// A serve link adapter, independent of local business RPC connections.
pub trait Center: fmt::Debug + Send + Sync {
    /// Observe authority, physical connection and discovered capability changes.
    fn state(&self) -> watch::Receiver<LinkState>;

    /// Call `method` only under `scope`, returning the result or a classified failure.
    /// Pending calls survive physical resume; a new logical link requires an idempotent retry.
    fn call(&self, scope: Scope, method: &'static str, params: Value) -> CallFuture<'_>;
}

/// Host-owned post-commit observer, invoked before connection focus or subscription filtering.
pub trait EventPublisher: fmt::Debug + Send + Sync {
    /// Offer an immutable committed event without blocking on network I/O.
    /// Return true only when the center route owns this event's native notification;
    /// local native push must then be suppressed. Ordinary client observers still receive state.
    fn publish(&self, method: &str, params: &Value) -> bool;
}

/// Safe publication journal failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Host publication journal unavailable or invalid")]
pub struct StoreError;

/// Blocking atomic storage for the daemon-owned publication journal schema.
pub trait OutboxStore: fmt::Debug + Send + Sync {
    /// Read a bounded journal; missing storage returns null.
    /// # Errors
    /// Returns a safe I/O or malformed-document error without modifying existing storage.
    fn load(&self) -> Result<Value, StoreError>;
    /// Atomically replace the journal before acknowledging an enqueue or removal.
    /// # Errors
    /// Returns a safe I/O error; the previous committed document remains usable.
    fn save(&self, document: &Value) -> Result<(), StoreError>;
}

#[cfg(test)]
mod tests;
