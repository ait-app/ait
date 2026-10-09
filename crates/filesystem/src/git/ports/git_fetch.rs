//! Blocking, cancellable remote Git operations for background observations.

use std::fmt::Debug;
use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use super::checkout::{CheckoutRuntimeError, CheckoutStatus};

/// Sanitized background Git failure; remote URLs and credential diagnostics are not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GitFetchError {
    /// Git or filesystem inspection failed.
    #[error("background Git operation failed")]
    Failed,
    /// The fetch exceeded its execution deadline.
    #[error("background Git fetch timed out")]
    TimedOut,
    /// The last observer or the server released the operation.
    #[error("background Git fetch cancelled")]
    Cancelled,
}

/// Remote reads implemented by a local Git adapter, independent of foreground checkout locks.
pub trait GitFetchRuntime: Debug + Send + Sync {
    /// Resolve the canonical shared Git directory, or return None for non-Git/no-origin paths.
    /// # Errors
    /// Returns a sanitized Git or filesystem failure.
    fn repository(&self, cwd: &str) -> Result<Option<PathBuf>, GitFetchError>;

    /// Fetch origin with pruning, without changing HEAD, the index or working files.
    /// # Errors
    /// Returns cancellation, timeout or sanitized command failure.
    fn fetch(&self, cwd: &str, cancellation: &CancellationToken) -> Result<(), GitFetchError>;

    /// Read the post-fetch checkout status for one observed directory.
    /// # Errors
    /// Returns the existing categorized checkout read error.
    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError>;
}
