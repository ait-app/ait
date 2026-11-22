//! Persistence boundary for Workspace notepads (ADR-296, ADR-297).

use std::fmt::Debug;
use std::path::Path;

use domain::workspace::notepad::Notepad;

/// Why a notepad could not be read or written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotepadStoreError {
    /// The storage volume has no space left.
    #[error("notepad storage is full")]
    Full,
    /// The file could not be read or written, or does not hold text a notepad can hold.
    #[error("notepad storage failed")]
    Io,
    /// The file no longer holds what the writer read; nothing was written.
    #[error("the notepad file changed while it was written")]
    Changed,
}

/// Blocking store of every Workspace's notepad file; hosts serialize calls outside an async
/// reactor. People edit the files directly, so a file can change between any two calls.
pub trait NotepadStore: Debug + Send + Sync {
    /// Read the notepad file of `workspace_id`, creating it first when it does not exist: with
    /// the text of the file at `legacy` when that is a non-empty text a notepad can hold, and
    /// empty otherwise.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the file cannot be created or read or holds invalid
    /// data.
    fn open(&self, workspace_id: &str, legacy: Option<&Path>)
    -> Result<Notepad, NotepadStoreError>;

    /// Atomically replace the text of the notepad file of `workspace_id` with `text`, provided
    /// the file still holds what `read` held.
    ///
    /// # Returns
    /// The notepad as written.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError::Changed`] when the file changed or disappeared since `read`,
    /// and the other errors when nothing could be written.
    fn save(
        &self,
        workspace_id: &str,
        read: &Notepad,
        text: &str,
    ) -> Result<Notepad, NotepadStoreError>;

    /// Remove the notepad file of `workspace_id`.
    ///
    /// # Errors
    /// Returns [`NotepadStoreError`] when the notepad could not be removed.
    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError>;

    /// The Workspace whose notepad file is kept directly in `directory`, whether or not that
    /// Workspace or file exists.
    fn owner(&self, directory: &Path) -> Option<String>;
}
