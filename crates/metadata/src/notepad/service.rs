//! Serialized notepad use cases over the notepad store and the Workspace registry.

use std::fmt;
use std::sync::Arc;

use domain::workspace::notepad::{Notepad, NotepadError, Write};
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
    /// The notepad refused the write; `current` is its unchanged state.
    #[error("{reason}")]
    Refused {
        /// Why the write was refused.
        reason: NotepadError,
        /// The notepad as it still is.
        current: Box<Notepad>,
    },
}

/// The notepad of every active Workspace. A Workspace has its notepad from creation, empty until
/// first written; it is kept while the Workspace is archived and dropped when it is removed.
pub struct WorkspaceNotepads {
    store: Arc<dyn NotepadStore>,
    workspaces: Box<dyn WorkspaceRegistry>,
    _removals: Box<dyn MutationSubscription>,
}

impl fmt::Debug for WorkspaceNotepads {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WorkspaceNotepads")
            .finish_non_exhaustive()
    }
}

impl WorkspaceNotepads {
    /// Serve notepads from `store` for the Workspaces of `workspaces`, dropping a notepad when
    /// its Workspace is removed.
    #[must_use]
    pub fn new(store: Arc<dyn NotepadStore>, workspaces: Box<dyn WorkspaceRegistry>) -> Self {
        let cleared = store.clone();
        let removals = workspaces.subscribe_to_mutations(Arc::new(move |mutation| {
            if mutation.kind == MutationKind::Remove
                && let Err(error) = cleared.clear(&mutation.workspace_id)
            {
                tracing::warn!(workspace_id = %mutation.workspace_id, %error,
                    "could not drop the notepad of a removed workspace");
            }
            Ok(())
        }));
        Self {
            store,
            workspaces,
            _removals: removals,
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
    /// The notepad after the write.
    /// # Errors
    /// Returns the failures of [`Self::notepad`], a refusal carrying the unchanged notepad, or a
    /// storage failure, after which nothing was written.
    pub(crate) fn write(
        &self,
        workspace_id: &str,
        write: Write<'_>,
    ) -> Result<Notepad, NotepadFailure> {
        let mut notepad = self.notepad(workspace_id)?;
        if let Err(reason) = notepad.write(write) {
            return Err(NotepadFailure::Refused {
                reason,
                current: Box::new(notepad),
            });
        }
        self.store.save(workspace_id, &notepad)?;
        Ok(notepad)
    }
}

#[cfg(test)]
pub(crate) mod tests;
