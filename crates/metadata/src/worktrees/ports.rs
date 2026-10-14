//! Host-owned adapter boundaries.
//!
//! Host-owned cleanup that must finish before an archived managed worktree is removed.

use std::fmt::Debug;

use domain::workspace::worktrees::WorktreeError;

/// Host-owned process cleanup required before removing an archived Workspace checkout.
pub trait WorktreeArchiveCleanup: Debug + Send + Sync {
    /// Close resources for the selected Workspace identities, including archived retry targets.
    /// # Errors
    /// Returns a retryable cleanup failure; the worktree service must retain the checkout.
    fn close_workspaces(&self, workspace_ids: &[String]) -> Result<(), WorktreeError>;
}
