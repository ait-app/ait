//! Shared naming input and the Git branch rename boundary.

use std::fmt::Debug;

/// Blocking Git boundary for renaming a still-eligible managed placeholder branch.
pub trait WorkspaceBranchNamer: Debug + Send + Sync {
    /// Rename only when `cwd` remains managed and its current branch equals `expected`.
    /// Returns the chosen collision-free branch, or none when no safe rename is possible.
    fn rename(&self, cwd: &str, expected: &str, desired: &str) -> Option<String>;
}
