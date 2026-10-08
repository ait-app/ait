//! Connection event and presence values, without live observers or delivery resources.

/// Connection event and presence payloads.
pub mod protocol;

/// Safe subscription validation or delivery failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    /// Invalid heartbeat or subscription parameters.
    #[error("invalid session parameters")]
    Invalid,
    /// The requested producer has not been implemented.
    #[error("unsupported session event")]
    Unsupported,
    /// Delivery closed or exhausted its bounded pending queue.
    #[error("session event delivery closed")]
    Closed,
}
