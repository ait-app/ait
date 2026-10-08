//! Worktree provisioning intents, registered placement and safe creation failures.

use crate::workspace::records::{PersistedProjectRecord, PersistedWorkspaceRecord};

/// Branch selection independent of the wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeAction {
    /// Create a branch from a selected base.
    BranchOff,
    /// Check out an existing branch.
    Checkout,
}

/// Legacy Agent Git placement that changes the source checkout without creating a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryGit {
    /// Create a normalized branch from an explicit or repository-default base, without tracking.
    BranchOff {
        /// Requested branch name seed.
        branch: String,
        /// Base branch; absence resolves the repository default.
        base: Option<String>,
    },
    /// Switch to an existing local or remote-tracking branch.
    Checkout {
        /// Existing branch reference.
        branch: String,
    },
}

/// Forge-neutral pull request selected for a managed checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeChangeRequest {
    /// Explicit forge, or the repository's default.
    pub forge: Option<String>,
    /// Positive JavaScript-safe request number.
    pub number: u64,
    /// Optional forge project identity retained from the request.
    pub project_path: Option<String>,
}

/// Creation intent, including the identity reserved by the creation coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreation {
    /// Source directory, or the selected project's root when omitted.
    pub cwd: Option<String>,
    /// Explicit active owning project.
    pub project_id: Option<String>,
    /// Reserved workspace identity.
    pub workspace_id: Option<String>,
    /// Explicit workspace title.
    pub title: Option<String>,
    /// Managed directory name seed.
    pub worktree_slug: Option<String>,
    /// Base or checkout reference.
    pub ref_name: Option<String>,
    /// Default base override when no reference is selected.
    pub base_branch: Option<String>,
    /// Explicit new branch name, independent of the directory name.
    pub branch_name: Option<String>,
    /// Branch creation or checkout.
    pub action: WorktreeAction,
    /// Optional pull or merge request checkout source.
    pub checkout_source: Option<WorktreeChangeRequest>,
    /// Provisional title source when no explicit title was supplied.
    pub first_agent_prompt: Option<String>,
    /// Whether an initial Agent will follow creation.
    pub expects_initial_agent: bool,
}

/// Registered workspace and its owning project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedWorktreeWorkspace {
    /// Persisted workspace placement.
    pub workspace: PersistedWorkspaceRecord,
    /// Project used for descriptor projection.
    pub project: PersistedProjectRecord,
}

/// Safe provisioning failure returned to the creation workflow.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct WorktreeCreationError {
    /// Stable wire error category.
    pub code: &'static str,
    /// Safe business error description.
    pub message: String,
}
