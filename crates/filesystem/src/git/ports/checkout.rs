//! Blocking Git checkout read boundary.

/// Categorized checkout read failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckoutFailureKind {
    /// Directory is not a Git checkout.
    NotGitRepository,
    /// Input or path is not allowed.
    NotAllowed,
    /// Checkout contains unresolved conflicts.
    MergeConflict,
    /// Git, filesystem, timeout, or output failure.
    Unknown,
}

/// Checkout adapter failure with a local diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct CheckoutRuntimeError {
    /// Stable error category.
    pub(crate) kind: CheckoutFailureKind,
    /// Human-readable local diagnostic.
    pub(crate) message: String,
}

/// Ahead/behind counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AheadBehind {
    /// Commits reachable only from HEAD.
    pub ahead: u64,
    /// Commits reachable only from the comparison ref.
    pub behind: u64,
}

/// Facts for the workspace action button, independent of the configured upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckoutBranchStatus {
    /// Current checkout commit, absent before the first commit.
    pub(crate) head_sha: Option<String>,
    /// Whether the index contains unresolved merge conflicts.
    pub(crate) has_conflicts: bool,
    /// Same-named branch on the preferred remote, absent if it does not exist locally.
    pub(crate) remote_ref: Option<String>,
    /// Counts against the same-named remote branch, absent without that branch.
    pub(crate) ahead_behind: Option<AheadBehind>,
}

/// Git/non-Git checkout status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutStatus {
    /// Whether Git metadata was found.
    pub(crate) is_git: bool,
    /// Worktree root.
    pub(crate) repo_root: Option<String>,
    /// Main repository root for linked worktrees.
    pub(crate) main_repo_root: Option<String>,
    /// Current branch.
    pub(crate) current_branch: Option<String>,
    /// Working tree dirtiness.
    pub(crate) is_dirty: Option<bool>,
    /// Workspace action facts, absent outside Git or from older adapters.
    pub(crate) branch_status: Option<CheckoutBranchStatus>,
    /// Display comparison base.
    pub(crate) base_ref: Option<String>,
    /// Counts against the comparison base.
    pub(crate) ahead_behind: Option<AheadBehind>,
    /// Exact configured upstream ref.
    pub(crate) upstream_ref: Option<String>,
    /// Commits ahead of upstream.
    pub(crate) ahead_of_origin: Option<u64>,
    /// Commits behind upstream.
    pub(crate) behind_of_origin: Option<u64>,
    /// Whether any remote exists.
    pub(crate) has_remote: bool,
    /// Preferred remote URL.
    pub(crate) remote_url: Option<String>,
    /// Whether the checkout is below the server-managed worktree root.
    pub(crate) is_managed_worktree: bool,
}

/// Diff comparison mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckoutDiffMode {
    /// Working tree and index against HEAD.
    Uncommitted,
    /// HEAD against a branch merge base.
    Base,
}

/// Diff comparison request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutDiffCompare {
    /// Comparison mode.
    pub(crate) mode: CheckoutDiffMode,
    /// Optional explicit base.
    pub(crate) base_ref: Option<String>,
    /// Ignore whitespace-only changes.
    pub(crate) ignore_whitespace: bool,
}

/// Structured line category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    /// Added line.
    Add,
    /// Removed line.
    Remove,
    /// Context line.
    Context,
    /// Hunk header.
    Header,
}

/// Theme-independent syntax token produced by a checkout adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HighlightToken {
    /// Source text without a diff marker.
    pub(crate) text: String,
    /// Optional syntax role understood by the client palette.
    pub(crate) style: Option<String>,
}

/// One structured diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffLine {
    /// Category.
    pub(crate) kind: DiffLineKind,
    /// Content without prefix.
    pub(crate) content: String,
    /// Optional syntax tokens; absent for unsupported or oversized content.
    pub(crate) tokens: Option<Vec<HighlightToken>>,
}

/// One structured hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffHunk {
    /// First old line.
    pub(crate) old_start: u64,
    /// Old line count.
    pub(crate) old_count: u64,
    /// First new line.
    pub(crate) new_start: u64,
    /// New line count.
    pub(crate) new_count: u64,
    /// Header and body lines.
    pub(crate) lines: Vec<DiffLine>,
}

/// Structured diff status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParsedDiffStatus {
    /// Binary placeholder.
    Binary,
}

/// One structured file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiffFile {
    /// Destination path.
    pub(crate) path: String,
    /// Source path for a rename.
    pub(crate) old_path: Option<String>,
    /// New-file marker.
    pub(crate) is_new: bool,
    /// Deleted-file marker.
    pub(crate) is_deleted: bool,
    /// Added lines.
    pub(crate) additions: u64,
    /// Removed lines.
    pub(crate) deletions: u64,
    /// Parsed hunks.
    pub(crate) hunks: Vec<DiffHunk>,
    /// Optional placeholder status.
    pub(crate) status: Option<ParsedDiffStatus>,
}

/// Checkout diff snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutDiff {
    /// Path-sorted files.
    pub(crate) files: Vec<ParsedDiffFile>,
    /// Whether the aggregate diff exceeded its budget.
    pub(crate) diff_too_large: bool,
}

/// Commit file status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckoutCommitFileStatus {
    /// Added.
    Added,
    /// Modified/type changed.
    Modified,
    /// Deleted.
    Deleted,
    /// Renamed.
    Renamed,
}

/// File statistics attached to a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckoutCommitFile {
    /// Destination path.
    pub(crate) path: String,
    /// Added lines.
    pub(crate) additions: u64,
    /// Removed lines.
    pub(crate) deletions: u64,
    /// Optional Git status.
    pub(crate) status: Option<CheckoutCommitFileStatus>,
}

/// One checkout commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckoutCommit {
    /// Full SHA.
    pub(crate) sha: String,
    /// Short SHA.
    pub(crate) short_sha: String,
    /// Subject.
    pub(crate) subject: String,
    /// Author name.
    pub(crate) author_name: String,
    /// ISO timestamp.
    pub(crate) author_date: String,
    /// Reachable from a remote ref.
    pub(crate) is_on_remote: bool,
    /// Belongs to bounded base context.
    pub(crate) is_on_base: bool,
    /// Changed files.
    pub(crate) files: Vec<CheckoutCommitFile>,
}

/// Commit history result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutCommits {
    /// Resolved comparison base.
    pub(crate) base_ref: Option<String>,
    /// Workspace commits followed by base context.
    pub(crate) commits: Vec<CheckoutCommit>,
}

/// Existing branch resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutBranchResolution {
    /// Existing local branch.
    Local(String),
    /// Existing origin tracking ref without a local branch.
    RemoteOnly {
        /// Normalized local branch name.
        name: String,
        /// Exact origin-qualified source ref.
        remote_ref: String,
    },
    /// No matching branch.
    NotFound,
}

/// Existing branch checkout source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutBranchSource {
    /// Existing local branch.
    Local,
    /// Origin-only branch materialized locally.
    Remote,
}

/// Branch suggestion facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutBranchSuggestion {
    /// Normalized local name.
    pub(crate) name: String,
    /// Committer Unix timestamp.
    pub(crate) committer_date: i64,
    /// Whether a local branch exists.
    pub(crate) has_local: bool,
    /// Whether an origin ref exists.
    pub(crate) has_remote: bool,
    /// Commits present only locally.
    pub(crate) local_ahead: Option<u64>,
    /// Commits present only on origin.
    pub(crate) local_behind: Option<u64>,
}

/// Merge-current-branch-to-base strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckoutMergeStrategy {
    /// Ordinary merge.
    Merge,
    /// Squash and commit.
    Squash,
}

/// One parsed stash entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckoutStashEntry {
    /// Zero-based stash index.
    pub(crate) index: usize,
    /// Full Git subject.
    pub(crate) message: String,
    /// Ait auto-stash branch label.
    pub(crate) branch: Option<String>,
    /// Whether the subject carries the Ait prefix; field name retained for wire compatibility.
    pub(crate) is_paseo: bool,
}

/// Blocking Git checkout runtime.
pub trait CheckoutRuntime: std::fmt::Debug + Send + Sync {
    /// Inspect checkout status.
    ///
    /// # Errors
    /// Returns categorized path, Git, timeout, or output failures.
    fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError>;

    /// Force a complete checkout read. Stateless adapters may delegate to status.
    ///
    /// # Errors
    /// Returns categorized path, Git, timeout, or output failures.
    fn refresh(&self, cwd: &str) -> Result<(), CheckoutRuntimeError>;

    /// Produce a structured diff.
    ///
    /// # Errors
    /// Returns categorized input, Git, timeout, or output failures.
    fn diff(
        &self,
        cwd: &str,
        compare: &CheckoutDiffCompare,
    ) -> Result<CheckoutDiff, CheckoutRuntimeError>;

    /// List branch history and bounded base context.
    ///
    /// # Errors
    /// Returns categorized Git, timeout, or output failures.
    fn commits(&self, cwd: &str) -> Result<CheckoutCommits, CheckoutRuntimeError>;

    /// Read one textual file diff from a commit.
    ///
    /// # Errors
    /// Returns categorized input, Git, timeout, or output failures.
    fn commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        path: &str,
    ) -> Result<Option<ParsedDiffFile>, CheckoutRuntimeError>;

    /// Resolve a local or origin branch.
    ///
    /// # Errors
    /// Returns categorized validation, Git, timeout, or output failures.
    fn validate_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<CheckoutBranchResolution, CheckoutRuntimeError>;

    /// List matching local and origin branches.
    ///
    /// # Errors
    /// Returns categorized validation, Git, timeout, or output failures.
    fn branch_suggestions(
        &self,
        cwd: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CheckoutBranchSuggestion>, CheckoutRuntimeError>;

    /// Check out an existing local or origin-only branch.
    ///
    /// # Errors
    /// Returns categorized dirty-tree, validation, Git, timeout, or output failures.
    fn switch_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<CheckoutBranchSource, CheckoutRuntimeError>;

    /// Rename the current local branch.
    ///
    /// # Errors
    /// Returns categorized detached-head, validation, Git, timeout, or output failures.
    fn rename_branch(&self, cwd: &str, branch: &str) -> Result<String, CheckoutRuntimeError>;

    /// Commit the current index, optionally staging all changes first.
    ///
    /// # Errors
    /// Returns categorized validation, Git, timeout, or output failures.
    fn commit(&self, cwd: &str, message: &str, add_all: bool) -> Result<(), CheckoutRuntimeError>;

    /// Merge the current branch into its base checkout.
    ///
    /// # Errors
    /// Returns categorized preflight, conflict, Git, timeout, or output failures.
    fn merge_to_base(
        &self,
        cwd: &str,
        base_ref: Option<&str>,
        strategy: CheckoutMergeStrategy,
        require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError>;

    /// Merge the selected base into the current branch.
    ///
    /// # Errors
    /// Returns categorized preflight, conflict, Git, timeout, or output failures.
    fn merge_from_base(
        &self,
        cwd: &str,
        base_ref: Option<&str>,
        require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError>;

    /// Fetch origin's default branch, restore the workspace's initial branch name, and reset HEAD.
    /// Force-push the reset HEAD if origin has a branch with that initial name.
    ///
    /// # Arguments
    /// * `cwd` - Directory of the managed worktree to reset.
    /// * `initial_branch` - Branch name saved when the workspace was created.
    ///
    /// # Errors
    /// Returns categorized validation, remote, Git, timeout, or output failures. A push failure
    /// leaves the local reset in place and reports that the remote reset did not complete.
    fn reset_workspace(&self, cwd: &str, initial_branch: &str) -> Result<(), CheckoutRuntimeError>;

    /// Pull the current branch.
    ///
    /// # Errors
    /// Returns categorized remote, conflict, Git, timeout, or output failures.
    fn pull(&self, cwd: &str) -> Result<(), CheckoutRuntimeError>;

    /// Push the current branch.
    ///
    /// # Errors
    /// Returns categorized remote, Git, timeout, or output failures.
    fn push(&self, cwd: &str) -> Result<(), CheckoutRuntimeError>;

    /// Restore tracked changes and remove selected untracked paths.
    ///
    /// # Errors
    /// Returns categorized path, Git, timeout, or output failures.
    fn discard_changes(&self, cwd: &str, paths: &[String]) -> Result<(), CheckoutRuntimeError>;

    /// Save tracked and untracked changes with the Ait stash prefix.
    ///
    /// # Errors
    /// Returns categorized Git, timeout, or output failures.
    fn stash_save(&self, cwd: &str, branch: Option<&str>) -> Result<(), CheckoutRuntimeError>;

    /// Pop one stash index.
    ///
    /// # Errors
    /// Returns categorized conflict, Git, timeout, or output failures.
    fn stash_pop(&self, cwd: &str, index: usize) -> Result<(), CheckoutRuntimeError>;

    /// List stashes, optionally filtering to Ait-created entries.
    ///
    /// # Errors
    /// Returns categorized Git, timeout, or output failures.
    fn stashes(
        &self,
        cwd: &str,
        paseo_only: bool,
    ) -> Result<Vec<CheckoutStashEntry>, CheckoutRuntimeError>;
}
