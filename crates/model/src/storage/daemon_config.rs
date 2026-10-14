//! Outbound daemon configuration persistence boundary.

use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use serde_json::Value;

/// Persistent, transactional storage for the daemon's mutable configuration.
pub trait DaemonConfigStore: Send + Sync + std::fmt::Debug {
    /// Return the current in-memory configuration, initializing storage if absent.
    ///
    /// # Errors
    /// Returns an error when the persisted document is invalid or unavailable.
    fn get(&self) -> Result<Value, DaemonConfigStoreError>;

    /// Merge one validated Paseo mutable patch and persist before publishing it.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read, validated, or replaced.
    fn patch(&self, patch: &Value) -> Result<Value, DaemonConfigStoreError>;

    /// Reread externally edited configuration and classify changed paths.
    ///
    /// # Errors
    /// Returns an error when the persisted document is invalid or unavailable.
    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError>;
}
