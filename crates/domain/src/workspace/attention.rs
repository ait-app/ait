//! Workspace activity contributions, attention results and safe state failures.

use crate::workspace::activity::WorkspaceStateBucket;

/// One activity contribution attributed to an explicit Workspace identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceActivity {
    /// Owning Workspace; directory equality does not imply shared ownership.
    pub workspace_id: String,
    /// Activity status before aggregation with other contributors.
    pub bucket: WorkspaceStateBucket,
    /// Best known entry time into this status, when the source can supply one.
    pub changed_at: Option<String>,
}

/// Workspace state operation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceStateError {
    /// Workspace is missing or archived for an attention mutation.
    #[error("Workspace not found: {0}")]
    WorkspaceNotFound(String),
    /// No eligible finished root Agent exists.
    #[error("Workspace has no finished agent to mark unread: {0}")]
    NoFinishedAgent(String),
    /// The selected Agent changed after candidate selection.
    #[error("Agent is no longer finished and read: {0}")]
    AgentNoLongerFinished(String),
    /// Agent runtime persistence failed.
    #[error("Agent runtime registry failed")]
    AgentRegistry,
    /// Workspace registry persistence failed.
    #[error("Workspace registry failed")]
    WorkspaceRegistry,
}

/// Durable changes completed before an optional per-Workspace failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceAttentionChanges {
    /// Agent identities in the original registry order.
    pub cleared_agent_ids: Vec<String>,
    /// Failure after any preceding updates have committed.
    pub error: Option<WorkspaceStateError>,
}
