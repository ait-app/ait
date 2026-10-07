//! Durable schedule storage contracts.

use crate::schedule::Schedule;

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

/// Durable full-state replacement; failed writes must preserve the prior document.
pub trait Store: Send + std::fmt::Debug {
    /// Load durable schedules. Missing storage is an empty list.
    /// # Errors
    /// Returns storage errors for unreadable or invalid documents.
    fn load(&self) -> Result<Vec<Schedule>, Error>;
    /// Atomically replace storage with validated schedules.
    /// # Errors
    /// Returns storage errors without publishing a partial document.
    fn save(&mut self, schedules: &[Schedule]) -> Result<(), Error>;
}
