//! Durable schedule storage contracts.

use domain::schedule::Schedule;
use domain::storage::schedule::Error;

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
