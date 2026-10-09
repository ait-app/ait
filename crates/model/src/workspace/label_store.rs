//! Persistence boundary for atomic workspace label catalog and assignment changes.

use std::fmt::Debug;

use domain::workspace::label_store::{
    WorkspaceLabelStoreError, WorkspaceLabelStoreMutation, WorkspaceLabelStoreSnapshot,
};

/// Blocking store; hosts serialize calls outside an async reactor.
pub trait WorkspaceLabelStore: Debug + Send + Sync {
    /// Load and recover any interrupted compound transaction.
    ///
    /// # Errors
    /// Returns invalid, I/O, or uncertain storage errors.
    fn initialize(&self) -> Result<(), WorkspaceLabelStoreError>;

    /// Return one coherent catalog/workspace snapshot.
    ///
    /// # Errors
    /// Returns invalid, I/O, or uncertain storage errors.
    fn snapshot(&self) -> Result<WorkspaceLabelStoreSnapshot, WorkspaceLabelStoreError>;

    /// Atomically publish a catalog and all assignment rewrites.
    ///
    /// # Errors
    /// Returns a missing/archived assignment target, conflict, invalid, I/O,
    /// or uncertain storage errors.
    fn commit(
        &self,
        mutation: &WorkspaceLabelStoreMutation,
    ) -> Result<(), WorkspaceLabelStoreError>;
}
