//! Persistence boundary for Workspace notepads (ADR-296).

use std::fmt::Debug;

use domain::workspace::notepad::{NoteChange, Notepad};

/// Why a notepad could not be read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotepadStoreError {
    /// The storage volume has no space left.
    #[error("notepad storage is full")]
    Full,
    /// The store could not be read or written, or holds invalid data.
    #[error("notepad storage failed")]
    Io,
}

/// Blocking store of every Workspace's notepad; hosts serialize calls outside an async reactor.
pub trait NotepadStore: Debug + Send + Sync {
    /// Load the notepad of `workspace_id`; a Workspace without notes has an empty notepad.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the store cannot be read or holds invalid data.
    fn load(&self, workspace_id: &str) -> Result<Notepad, NotepadStoreError>;

    /// Atomically persist `change` to the notepad of `workspace_id` together with its new
    /// `revision`.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when nothing was written.
    fn commit(
        &self,
        workspace_id: &str,
        revision: u64,
        change: &NoteChange,
    ) -> Result<(), NotepadStoreError>;

    /// Remove the notepad of `workspace_id` with all its notes.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the notepad could not be removed.
    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError>;
}
