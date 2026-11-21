//! Serialized notepad use cases over the notepad store and the Workspace registry.

use std::fmt;
use std::sync::Arc;

use domain::workspace::notepad::{Note, NoteChange, Notepad, NotepadError, Write};
use domain::workspace::registry::MutationKind;
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use model::workspace::registry::{MutationSubscription, WorkspaceRegistry};

/// Why a notepad request did not complete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotepadFailure {
    /// The Workspace is unknown or archived, or the registry could not be read.
    #[error("workspace not found")]
    Workspace,
    /// The store could not be read or written.
    #[error(transparent)]
    Storage(#[from] NotepadStoreError),
    /// The notepad refused the change.
    #[error(transparent)]
    Refused(#[from] NotepadError),
}

/// Notepads of active Workspaces; an archived or removed Workspace loses its notepad.
pub struct WorkspaceNotepads {
    store: Arc<dyn NotepadStore>,
    workspaces: Box<dyn WorkspaceRegistry>,
    _archives: Box<dyn MutationSubscription>,
}

impl fmt::Debug for WorkspaceNotepads {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceNotepads")
            .finish_non_exhaustive()
    }
}

impl WorkspaceNotepads {
    /// Serve notepads from `store` for the Workspaces of `workspaces`, clearing a notepad when
    /// its Workspace is archived or removed.
    #[must_use]
    pub fn new(store: Arc<dyn NotepadStore>, workspaces: Box<dyn WorkspaceRegistry>) -> Self {
        let cleared = store.clone();
        let archives = workspaces.subscribe_to_mutations(Arc::new(move |mutation| {
            if matches!(mutation.kind, MutationKind::Archive | MutationKind::Remove)
                && let Err(error) = cleared.clear(&mutation.workspace_id)
            {
                tracing::warn!(workspace_id = %mutation.workspace_id, %error,
                    "could not clear the notepad of a retired workspace");
            }
            Ok(())
        }));
        Self {
            store,
            workspaces,
            _archives: archives,
        }
    }

    /// Load the notepad of the active Workspace `workspace_id`.
    /// # Errors
    /// Returns [`NotepadFailure::Workspace`] for an unknown or archived Workspace and
    /// [`NotepadFailure::Storage`] when the store cannot be read.
    pub(crate) fn notepad(&self, workspace_id: &str) -> Result<Notepad, NotepadFailure> {
        let active = self
            .workspaces
            .get(workspace_id)
            .map_err(|_| NotepadFailure::Workspace)?
            .is_some_and(|workspace| workspace.archived_at.is_none());
        if !active {
            return Err(NotepadFailure::Workspace);
        }
        Ok(self.store.load(workspace_id)?)
    }

    /// Apply `write` to the notepad of `workspace_id` and persist it.
    /// # Returns
    /// The written note.
    /// # Errors
    /// Returns the failures of [`Self::notepad`], a refusal of the write, or a storage failure,
    /// after which nothing was written.
    pub(crate) fn write(
        &self,
        workspace_id: &str,
        write: Write<'_>,
    ) -> Result<Note, NotepadFailure> {
        let mut notepad = self.notepad(workspace_id)?;
        let change = notepad.write(write)?;
        self.store
            .commit(workspace_id, notepad.revision(), &change)?;
        match change {
            NoteChange::Put(note) => Ok(note),
            NoteChange::Remove(_) => Err(NotepadFailure::Refused(NotepadError::Missing)),
        }
    }

    /// Delete the note `key` of `workspace_id` when its revision equals `expected`.
    /// # Returns
    /// The notepad revision after the deletion.
    /// # Errors
    /// Returns the failures of [`Self::notepad`], a refusal of the deletion, or a storage
    /// failure, after which nothing was deleted.
    pub(crate) fn delete(
        &self,
        workspace_id: &str,
        key: &str,
        expected: u64,
    ) -> Result<u64, NotepadFailure> {
        let mut notepad = self.notepad(workspace_id)?;
        let change = notepad.delete(key, expected)?;
        self.store
            .commit(workspace_id, notepad.revision(), &change)?;
        Ok(notepad.revision())
    }
}

#[cfg(test)]
pub(crate) mod tests;
