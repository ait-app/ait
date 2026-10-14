//! Worktree provisioning consumed by the unified Workspace creation workflow, and the local Git
//! adapters the worktree services coordinate.

use std::fmt::Debug;

use domain::workspace::worktrees::{
    ArchivedWorktreeRestore, ChangeRequestCheckout, CreatedManagedWorktree,
    CreatedWorktreeWorkspace, DirectoryGit, ManagedWorktreeCreate, ManagedWorktreeInfo,
    OwnedWorktree, WorkspaceRecoveryRuntimeError, WorktreeChangeRequest, WorktreeCreation,
    WorktreeCreationError, WorktreeError,
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

/// Blocking forge lookup that turns a selected change request into Git checkout facts.
pub trait ChangeRequestResolver: Debug + Send + Sync {
    /// Resolve `source` for the repository containing `cwd`, without mutating the checkout.
    /// `head_ref` overrides the local branch candidate when the caller already chose one.
    /// # Errors
    /// Returns unsupported forge, invalid request, authentication, or lookup failures.
    fn resolve_change_request(
        &self,
        cwd: &str,
        source: &WorktreeChangeRequest,
        head_ref: Option<&str>,
    ) -> Result<ChangeRequestCheckout, WorktreeError>;
}

/// Blocking adapter for server-owned linked Git checkouts.
pub trait ManagedWorktrees: Debug + Send + Sync {
    /// Resolve forge facts before choosing a worktree name or creating Git placement.
    /// # Errors
    /// Returns unsupported forge, invalid request, authentication, or lookup failures.
    fn resolve_change_request(
        &self,
        _cwd: &str,
        _source: &WorktreeChangeRequest,
        _head_ref: Option<&str>,
    ) -> Result<ChangeRequestCheckout, WorktreeError> {
        Err(WorktreeError::ForgeUnavailable)
    }

    /// Change a clean source checkout according to legacy Agent Git placement.
    ///
    /// # Errors
    /// Returns branch validation, dirtiness, or bounded Git execution failures.
    fn prepare_directory(&self, cwd: &str, intent: &DirectoryGit) -> Result<(), WorktreeError>;

    /// List managed worktrees belonging to the repository containing `cwd`.
    ///
    /// # Errors
    /// Returns a categorized Git or filesystem error.
    fn list(&self, cwd: &str) -> Result<Vec<ManagedWorktreeInfo>, WorktreeError>;

    /// Create one managed linked checkout and map the source-relative cwd into it.
    ///
    /// # Errors
    /// Returns a categorized validation, Git, or filesystem error. Failures after `git worktree
    /// add` make a best-effort rollback before returning.
    fn create(
        &self,
        input: &ManagedWorktreeCreate,
    ) -> Result<CreatedManagedWorktree, WorktreeError>;

    /// Resolve a path, possibly below a worktree root, into an ownership proof.
    ///
    /// # Errors
    /// Returns `NotAllowed` for paths outside the managed root.
    fn owned(&self, path: &str) -> Result<OwnedWorktree, WorktreeError>;

    /// Resolve a managed worktree path from repository and slug.
    ///
    /// # Errors
    /// Returns validation or Git inspection errors.
    fn path_for_slug(&self, repo_root: &str, slug: &str) -> Result<String, WorktreeError>;

    /// Return whether `candidate` is the root or a descendant of `root` after normalization.
    fn contains(&self, root: &str, candidate: &str) -> bool;

    /// Delete a proven managed worktree and prune stale Git administration state.
    ///
    /// # Errors
    /// Returns only after both Git and recursive filesystem removal fail to remove the target.
    fn remove(&self, worktree: &OwnedWorktree) -> Result<(), WorktreeError>;
}

/// Blocking local runtime used by archived Workspace recovery.
pub trait WorkspaceRecoveryRuntime: Debug + Send + Sync {
    /// Return true only when the exact path currently names a directory.
    fn is_directory(&self, path: &str) -> bool;

    /// Recreate the saved linked checkout and selected descendant directory.
    ///
    /// # Errors
    /// Returns a categorized validation, Git, or filesystem failure. An unsuccessful restore
    /// removes any worktree created during the attempt before returning where possible.
    fn restore_worktree(
        &self,
        input: &ArchivedWorktreeRestore,
    ) -> Result<(), WorkspaceRecoveryRuntimeError>;
}
