//! Serialized notepad use cases over the notepad store and the Workspace registry.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use domain::workspace::notepad::{Notepad, NotepadError, Write};
use domain::workspace::registry::MutationKind;
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use model::workspace::registry::{MutationSubscription, WorkspaceRegistry};

/// Notebook file earlier versions kept in the checkout; a new notepad starts with its text.
const LEGACY_NOTEBOOK: &str = ".ait-notebook.md";
/// Times a write is decided again after the file changed under it.
const WRITE_ATTEMPTS: u8 = 4;

/// Why a notepad request did not complete.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotepadFailure {
    /// The Workspace is unknown or archived, or the registry could not be read.
    #[error("workspace not found")]
    Workspace,
    /// The file could not be read or written, or kept changing under the write.
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

/// The notepad of every active Workspace: a file of the host outside the Workspace's checkout,
/// created empty when it is first asked for. It is kept while the Workspace is archived and
/// dropped when it is removed.
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

    /// Read the notepad of the active Workspace `workspace_id`, creating its file when absent;
    /// a new file starts with the notebook file earlier versions kept in the checkout.
    /// # Errors
    /// Returns [`NotepadFailure::Workspace`] for an unknown or archived Workspace and
    /// [`NotepadFailure::Storage`] when the file cannot be created or read.
    pub(crate) fn notepad(&self, workspace_id: &str) -> Result<Notepad, NotepadFailure> {
        let workspace = self
            .workspaces
            .get(workspace_id)
            .map_err(|_| NotepadFailure::Workspace)?
            .filter(|workspace| workspace.archived_at.is_none())
            .ok_or(NotepadFailure::Workspace)?;
        let legacy = Path::new(&workspace.cwd).join(LEGACY_NOTEBOOK);
        Ok(self.store.open(workspace_id, Some(&legacy))?)
    }

    /// Apply `write` to the notepad of `workspace_id` and replace its file.
    /// # Returns
    /// The notepad after the write.
    /// # Errors
    /// Returns the failures of [`Self::notepad`], a refusal carrying the unchanged notepad, or a
    /// storage failure, after which nothing was written.
    pub(crate) fn write(
        &self,
        workspace_id: &str,
        write: &Write<'_>,
    ) -> Result<Notepad, NotepadFailure> {
        let mut attempts = WRITE_ATTEMPTS;
        loop {
            let notepad = self.notepad(workspace_id)?;
            let text = match notepad.written(write) {
                Ok(text) => text,
                Err(reason) => {
                    return Err(NotepadFailure::Refused {
                        reason,
                        current: Box::new(notepad),
                    });
                }
            };
            attempts -= 1;
            match self.store.save(workspace_id, &notepad, &text) {
                // Someone edited the file meanwhile: decide again from what it holds now.
                Err(NotepadStoreError::Changed) if attempts > 0 => {}
                outcome => return Ok(outcome?),
            }
        }
    }

    /// The Workspace whose notepad file is kept directly in `directory`, whether or not that
    /// Workspace exists; file requests use it to tell a notepad directory from a checkout.
    #[must_use]
    pub fn workspace_of(&self, directory: &Path) -> Option<String> {
        self.store.owner(directory)
    }
}

#[cfg(test)]
pub(crate) mod tests;
