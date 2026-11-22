//! Persistence boundary for Workspace notepads (ADR-296).

use std::fmt::Debug;

use domain::workspace::notepad::Notepad;

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
    /// Load the notepad of `workspace_id`; a Workspace never written to has the empty notepad.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the store cannot be read or holds invalid data.
    fn load(&self, workspace_id: &str) -> Result<Notepad, NotepadStoreError>;

    /// Atomically replace the stored notepad of `workspace_id` with `notepad`.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when nothing was written.
    fn save(&self, workspace_id: &str, notepad: &Notepad) -> Result<(), NotepadStoreError>;

    /// Remove the stored notepad of `workspace_id`.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the notepad could not be removed.
    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError>;
}
