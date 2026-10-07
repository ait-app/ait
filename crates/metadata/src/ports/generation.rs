//! Summary consumption and Git effects used by Workspace naming.

use std::fmt::Debug;

use model::summary::{SummaryFuture, SummaryRequest};

/// Consumer boundary for host-supplied summaries; provider owns the generation capability.
pub trait SummarySource: Debug + Send + Sync {
    /// Generate the artifact in `request`, or return a safe budget/provider failure.
    /// # Errors
    /// Returns unavailable output, exhausted admission, or shutdown cancellation.
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_>;

    /// Cancel outstanding auxiliary work when the server drains. Does not cancel user turns.
    fn shutdown(&self);
}

/// Blocking Git boundary for renaming a still-eligible managed placeholder branch.
pub trait WorkspaceBranchNamer: Debug + Send + Sync {
    /// Rename only when `cwd` remains managed and its current branch equals `expected`.
    /// Returns the chosen collision-free branch, or none when no safe rename is possible.
    fn rename(&self, cwd: &str, expected: &str, desired: &str) -> Option<String>;
}
