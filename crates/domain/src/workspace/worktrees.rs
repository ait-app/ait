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

/// A managed worktree returned by Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWorktreeInfo {
    /// Absolute linked checkout root.
    pub path: String,
    /// Filesystem creation time in RFC 3339 form.
    pub created_at: String,
    /// Local branch, or none for detached HEAD.
    pub branch_name: Option<String>,
    /// Checked-out commit object name.
    pub head: Option<String>,
}

/// How a linked checkout obtains its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeCreateMode {
    /// Fetch and check out a resolved pull request without modifying an existing local branch.
    ChangeRequest(ChangeRequestCheckout),
    /// Create a new local branch from an optional base ref.
    BranchOff {
        /// Base ref; omission asks the adapter to resolve the repository default.
        base_ref: Option<String>,
        /// Desired local branch name.
        branch_name: String,
    },
    /// Check out an existing local or origin branch.
    Checkout {
        /// Existing branch name.
        branch_name: String,
    },
    /// Restore an archived worktree using its exact saved branch.
    Restore {
        /// Existing local branch; it must not be checked out elsewhere.
        branch_name: String,
        /// Saved comparison base, retained when it still resolves.
        base_ref: Option<String>,
    },
}

/// Resolved forge facts used for Git placement and automation trust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRequestCheckout {
    /// Resolved forge used for setup trust attribution.
    pub forge: String,
    /// Pull request number in the selected repository.
    pub number: u64,
    /// Actual source branch on the remote.
    pub head_ref: String,
    /// Base branch used for comparisons.
    pub base_ref: String,
    /// Local branch candidate; forks may prefix the head owner.
    pub local_branch: String,
    /// Fork identity requiring setup approval, absent for same-repository changes.
    pub untrusted_repository: Option<String>,
    /// Fork push URL, absent for same-repository changes.
    pub push_remote_url: Option<String>,
    /// Whether origin's source branch should be used for tracking.
    pub track_origin: bool,
    /// Ordered platform-specific refs, including any supported fallback.
    pub checkout_refs: Vec<ChangeRequestCheckoutRef>,
}

/// A forge-provided ref to try when fetching a change request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeRequestCheckoutRef {
    /// Named Git remote in the existing repository.
    pub remote: String,
    /// Full remote ref, validated before constructing a fetch refspec.
    pub reference: String,
}

/// Input to the atomic Git portion of managed worktree creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedWorktreeCreate {
    /// Source checkout directory, possibly below the repository root.
    pub cwd: String,
    /// Validated managed directory name.
    pub slug: String,
    /// Branch creation or checkout behavior.
    pub mode: WorktreeCreateMode,
}

/// Created Git placement before it is recorded in the workspace registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedManagedWorktree {
    /// Main repository root.
    pub repo_root: String,
    /// Canonical source directory supplied by the caller.
    pub source_cwd: String,
    /// Source-relative directory in the linked checkout.
    pub workspace_cwd: String,
    /// Linked checkout root.
    pub worktree_path: String,
    /// Actual local branch, including collision suffixes.
    pub branch_name: String,
    /// Comparison base retained in the workspace registry.
    pub comparison_base_ref: Option<String>,
    /// Preferred origin URL, when configured.
    pub remote_url: Option<String>,
}

/// Ownership proof for an archive target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedWorktree {
    /// Normalized managed worktree root, excluding a descendant workspace path.
    pub path: String,
    /// Main repository root when Git metadata is still available.
    pub repo_root: Option<String>,
}

/// Stable worktree adapter failure categories.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorktreeError {
    /// The selected path is outside a Git repository.
    #[error("Not in a git repository")]
    NotGitRepository,
    /// The target path is not inside the server-managed root.
    #[error("Worktree is not a Paseo-owned worktree")]
    NotAllowed,
    /// Checkout action omitted both a branch and change-request source.
    #[error("action \"checkout\" requires refName or checkoutSource")]
    MissingCheckoutTarget,
    /// An existing branch could not be resolved locally or from origin.
    #[error("Unknown branch: {0}")]
    UnknownBranch(String),
    /// A requested branch is already checked out and cannot be restored directly.
    #[error("Branch already checked out: {0}")]
    BranchAlreadyCheckedOut(String),
    /// A slug, branch, base ref, or path violates the worktree contract.
    #[error("{0}")]
    Invalid(String),
    /// A forge-backed checkout needs a service not installed in this phase.
    #[error("Change-request checkout requires a configured forge service")]
    ForgeUnavailable,
    /// Git or filesystem work failed.
    #[error("{0}")]
    Io(String),
}

/// Durable placement needed to recreate one archived managed worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchivedWorktreeRestore {
    /// Main repository that owns the linked checkout.
    pub source_repo_root: String,
    /// Exact deleted worktree root retained by the Workspace record.
    pub previous_worktree_root: String,
    /// Exact selected Workspace directory, possibly below the worktree root.
    pub workspace_cwd: String,
    /// Saved local branch to check out without inventing a replacement branch.
    pub branch: String,
    /// Saved comparison base, which may no longer resolve.
    pub base_ref: Option<String>,
}

/// Stable failures from local archived Workspace recovery.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceRecoveryRuntimeError {
    /// The saved branch is already checked out somewhere else.
    #[error("Branch already checked out: {0}")]
    BranchAlreadyCheckedOut(String),
    /// The saved branch cannot be resolved locally or from origin.
    #[error("Unknown branch: {0}")]
    UnknownBranch(String),
    /// Persisted placement cannot be restored safely.
    #[error("{0}")]
    Invalid(String),
    /// Git or filesystem work failed.
    #[error("{0}")]
    Io(String),
}
