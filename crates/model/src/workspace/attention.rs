//! Consumer-owned Workspace attention boundary without Agent record or Provider dependencies.

use std::fmt::Debug;

use domain::workspace::attention::{
    WorkspaceActivity, WorkspaceAttentionChanges, WorkspaceStateError,
};

/// Read-only activity projection consumed by the Workspace directory.
pub trait WorkspaceActivitySource: Debug + Send + Sync {
    /// Capture currently active contributions without changing Agent or Workspace state.
    ///
    /// # Errors
    /// Returns a categorized failure when the activity source cannot be read.
    fn snapshot(&self) -> Result<Vec<WorkspaceActivity>, WorkspaceStateError>;
}

/// One request's candidate snapshot, shared by all Workspaces in a clear-attention batch.
pub trait WorkspaceAttentionScan: Debug {
    /// Clear eligible attention using the captured candidates and return partial durable results.
    fn clear_attention(&self, workspace_id: &str, updated_at: &str) -> WorkspaceAttentionChanges;
}

/// Agent-owned attention operations consumed by Workspace services.
pub trait WorkspaceAttention: Debug + Send + Sync {
    /// Capture candidates once before processing a batch of Workspaces.
    ///
    /// # Errors
    /// Returns a categorized failure when Agent state cannot be read.
    fn scan(&self) -> Result<Box<dyn WorkspaceAttentionScan + '_>, WorkspaceStateError>;

    /// Mark the newest eligible finished root Agent in an already validated Workspace as unread.
    ///
    /// # Errors
    /// Returns a missing candidate, concurrent state change, or persistence failure.
    fn mark_unread(
        &self,
        workspace_id: &str,
        updated_at: &str,
    ) -> Result<String, WorkspaceStateError>;
}
