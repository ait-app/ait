//! Safe push token lease persistence failures.

/// Safe subscription failure, without token or filesystem contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PushError {
    /// Invalid input or persisted document.
    #[error("invalid push subscription")]
    Invalid,
    /// Persistence could not complete.
    #[error("push subscription storage failed")]
    Io,
    /// The bounded subscription store is full.
    #[error("push subscription capacity exhausted")]
    Capacity,
}
