//! Read-only checkout facts consumed by Workspace directory projections.

use std::fmt::Debug;

use domain::workspace::runtime::WorkspaceRuntimeSnapshot;

/// Consumer-owned source of ephemeral Workspace checkout facts.
pub trait WorkspaceRuntimeSource: Debug + Send + Sync {
    /// Return cached facts for `cwd` and request bounded asynchronous refresh when needed.
    ///
    /// This read must not wait for Git or network commands. Adapters own refresh admission,
    /// coalescing, and failures; directory snapshots remain available while facts warm.
    fn snapshot(&self, cwd: &str) -> WorkspaceRuntimeSnapshot;
}
