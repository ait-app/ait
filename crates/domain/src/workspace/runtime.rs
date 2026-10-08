//! Read-only Git, Forge and checkout snapshots for Workspace presentation.

/// Line counts against the checkout's comparison base, including working changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkspaceDiffStat {
    /// Added text lines, including untracked files.
    pub additions: u64,
    /// Removed text lines.
    pub deletions: u64,
}

/// Git facts shared by Workspaces using the same directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGitSnapshot {
    /// Current branch, or none for detached HEAD.
    pub current_branch: Option<String>,
    /// Preferred remote URL.
    pub remote_url: Option<String>,
    /// Whether the checkout is managed by this server.
    pub is_managed_worktree: bool,
    /// Working tree dirtiness.
    pub is_dirty: Option<bool>,
    /// Commits ahead of and behind the comparison base.
    pub ahead_behind: Option<(u64, u64)>,
    /// Commits ahead of the configured upstream.
    pub ahead_of_origin: Option<u64>,
    /// Commits behind the configured upstream.
    pub behind_of_origin: Option<u64>,
    /// Diff statistics, or none when unavailable or unchanged.
    pub diff_stat: Option<WorkspaceDiffStat>,
}

/// One normalized check used by the sidebar and Workspace hover card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCheckSnapshot {
    /// Check name.
    pub name: String,
    /// Normalized check lifecycle.
    pub status: String,
    /// Browser details URL.
    pub url: Option<String>,
    /// Workflow name.
    pub workflow: Option<String>,
    /// Formatted elapsed time.
    pub duration: Option<String>,
    /// Optional presentation refinements.
    pub traits: Option<Vec<String>>,
}

/// Current change request, independent of the filesystem adapter's types.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspacePullRequestSnapshot {
    /// Forge-local change request number.
    pub number: Option<u64>,
    /// Browser URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// Normalized open, closed, or merged state.
    pub state: String,
    /// Target branch.
    pub base_ref_name: String,
    /// Source branch.
    pub head_ref_name: String,
    /// Whether the change request has merged.
    pub is_merged: bool,
    /// Whether the change request is a draft.
    pub is_draft: bool,
    /// Normalized mergeability.
    pub mergeable: String,
    /// Individual CI checks.
    pub checks: Vec<WorkspaceCheckSnapshot>,
    /// Aggregate CI state.
    pub checks_status: String,
    /// Normalized review decision.
    pub review_decision: Option<String>,
    /// Repository owner.
    pub repo_owner: Option<String>,
    /// Repository name.
    pub repo_name: Option<String>,
    /// Optional provider-specific presentation facts.
    pub github: Option<serde_json::Value>,
}

/// Forge availability and latest change request facts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkspaceForgeSnapshot {
    /// Whether a supported and authenticated Forge is available.
    pub features_enabled: bool,
    /// Resolved platform brand.
    pub forge: Option<String>,
    /// Current change request, or none when absent or unavailable.
    pub pull_request: Option<WorkspacePullRequestSnapshot>,
    /// Latest bounded read failure, when present.
    pub error: Option<String>,
}

/// Cached checkout presentation facts; missing fields can still be warming.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkspaceRuntimeSnapshot {
    /// Latest Git facts, or none for a non-Git/missing directory.
    pub git: Option<WorkspaceGitSnapshot>,
    /// Latest Forge facts, independently refreshed from Git.
    pub forge: Option<WorkspaceForgeSnapshot>,
}
