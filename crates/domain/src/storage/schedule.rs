//! Safe schedule validation and persistence failures.

/// Stable errors that do not expose provider credentials or storage paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Invalid cadence, target or payload.
    #[error("Invalid schedule parameters")]
    Invalid,
    /// Unknown schedule ID.
    #[error("Schedule not found")]
    NotFound,
    /// Already running, completed or storage capacity reached.
    #[error("Schedule is busy, completed or capacity is exhausted")]
    Conflict,
    /// Atomic persistence or recovery failed.
    #[error("Schedule storage failed")]
    Storage,
}
