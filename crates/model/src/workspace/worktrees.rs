//! Worktree provisioning consumed by the unified Workspace creation workflow.

use std::fmt::Debug;

use domain::workspace::worktrees::{
    CreatedWorktreeWorkspace, DirectoryGit, WorktreeCreation, WorktreeCreationError,
};

/// Blocking adapter that creates Git placement and registers it atomically with rollback.
pub trait WorktreeProvisioning: Debug + Send + Sync {
    /// Prepare the source checkout for a legacy Agent creation request.
    ///
    /// # Errors
    /// Returns invalid branch, dirty checkout, Git, or persistence failures.
    fn prepare_directory(
        &self,
        cwd: &str,
        intent: &DirectoryGit,
    ) -> Result<(), WorktreeCreationError>;

    /// Archive an owned worktree Workspace and remove its unreferenced managed checkout.
    ///
    /// Only the filesystem owner resolves and validates the managed path.
    /// # Errors
    /// Returns ownership, Git cleanup, or persistence failures without removing unrelated paths.
    fn archive(&self, workspace_id: &str, timestamp: &str) -> Result<(), WorktreeCreationError>;

    /// Create the requested workspace using the reserved identity and timestamp.
    ///
    /// # Errors
    /// Returns validation, Git, registration, or rollback failures.
    fn create(
        &self,
        input: &WorktreeCreation,
        timestamp: &str,
    ) -> Result<CreatedWorktreeWorkspace, WorktreeCreationError>;
}
