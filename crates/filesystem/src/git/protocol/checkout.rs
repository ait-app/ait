//! Git checkout status, diff, and commit-history payloads.

use serde::{Deserialize, Serialize};

/// A checkout-scoped request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutPathRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
}

/// Stable Paseo checkout error categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CheckoutErrorCode {
    /// The directory is not in a Git repository.
    NotGitRepo,
    /// The requested read is outside the allowed checkout boundary.
    NotAllowed,
    /// The repository is in a conflicting state.
    MergeConflict,
    /// Another Git or filesystem failure occurred.
    Unknown,
}

/// Inline checkout error used by Paseo responses and updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckoutError {
    /// Stable machine-readable category.
    pub(crate) code: CheckoutErrorCode,
    /// Human-readable local diagnostic.
    pub(crate) message: String,
}

/// Ahead/behind counts for one comparison ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct AheadBehind {
    /// Commits reachable only from the checkout.
    pub ahead: u64,
    /// Commits reachable only from the comparison ref.
    pub behind: u64,
}

/// Workspace action facts compared with the same-named remote branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchStatus {
    /// Current commit, absent before the first commit.
    pub(crate) head_sha: Option<String>,
    /// Whether merge conflicts remain unresolved.
    pub(crate) has_conflicts: bool,
    /// Same-named remote-tracking ref, absent when not present.
    pub(crate) remote_ref: Option<String>,
    /// Counts against that ref, absent without a remote branch.
    pub(crate) ahead_behind: Option<AheadBehind>,
}

/// Checkout status response, including the non-Git null projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckoutStatusResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Whether Git metadata was found.
    pub(crate) is_git: bool,
    /// Checkout root, or null outside Git.
    pub(crate) repo_root: Option<String>,
    /// Main repository root for linked worktrees.
    pub(crate) main_repo_root: Option<String>,
    /// Current local branch, or null for detached HEAD/non-Git.
    pub(crate) current_branch: Option<String>,
    /// Working tree dirtiness, or null outside Git.
    pub(crate) is_dirty: Option<bool>,
    /// Additional facts for workspace actions.
    pub(crate) branch_status: Option<CheckoutBranchStatus>,
    /// Comparison base display name.
    pub(crate) base_ref: Option<String>,
    /// Counts against the comparison base.
    pub(crate) ahead_behind: Option<AheadBehind>,
    /// Exact upstream ref resolved by Git.
    pub(crate) upstream_ref: Option<String>,
    /// Commits ahead of the exact upstream.
    pub(crate) ahead_of_origin: Option<u64>,
    /// Commits behind the exact upstream.
    pub(crate) behind_of_origin: Option<u64>,
    /// Whether any remote is configured.
    pub(crate) has_remote: bool,
    /// Preferred remote URL.
    pub(crate) remote_url: Option<String>,
    /// Whether the checkout is below the independent server's managed worktree root.
    pub(crate) is_paseo_owned_worktree: bool,
    /// Inline read error.
    pub(crate) error: Option<CheckoutError>,
}

impl Serialize for CheckoutStatusResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(if self.is_git { 16 } else { 15 }))?;
        map.serialize_entry("cwd", &self.cwd)?;
        map.serialize_entry("isGit", &self.is_git)?;
        map.serialize_entry("repoRoot", &self.repo_root)?;
        if self.is_git {
            map.serialize_entry("mainRepoRoot", &self.main_repo_root)?;
        }
        map.serialize_entry("currentBranch", &self.current_branch)?;
        map.serialize_entry("isDirty", &self.is_dirty)?;
        map.serialize_entry("branchStatus", &self.branch_status)?;
        map.serialize_entry("baseRef", &self.base_ref)?;
        map.serialize_entry("aheadBehind", &self.ahead_behind)?;
        map.serialize_entry("upstreamRef", &self.upstream_ref)?;
        map.serialize_entry("aheadOfOrigin", &self.ahead_of_origin)?;
        map.serialize_entry("behindOfOrigin", &self.behind_of_origin)?;
        map.serialize_entry("hasRemote", &self.has_remote)?;
        map.serialize_entry("remoteUrl", &self.remote_url)?;
        map.serialize_entry("isPaseoOwnedWorktree", &self.is_paseo_owned_worktree)?;
        map.serialize_entry("error", &self.error)?;
        map.end()
    }
}

/// Checkout refresh result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutRefreshResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Whether the forced read completed.
    pub(crate) success: bool,
    /// Inline error.
    pub(crate) error: Option<CheckoutError>,
}

/// Diff comparison mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckoutDiffMode {
    /// Staged, unstaged, and untracked changes against HEAD.
    Uncommitted,
    /// Committed branch changes against a merge base.
    Base,
}

/// Diff comparison options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutDiffCompare {
    /// Comparison mode.
    pub(crate) mode: CheckoutDiffMode,
    /// Explicit base ref for base mode.
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    /// Ignore whitespace-only changes.
    #[serde(default)]
    pub(crate) ignore_whitespace: bool,
}

/// One-shot checkout diff request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutDiffGetRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Comparison options.
    pub(crate) compare: CheckoutDiffCompare,
}

/// Connection-owned checkout diff subscription request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutDiffSubscribeRequest {
    /// Optional caller-selected connection-local identity.
    #[serde(default)]
    pub(crate) subscription_id: Option<String>,
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Comparison options.
    pub(crate) compare: CheckoutDiffCompare,
}

/// Explicit checkout diff unsubscribe request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutDiffUnsubscribeRequest {
    /// Connection-local subscription identity.
    pub(crate) subscription_id: String,
}

/// Theme-independent syntax-highlight token produced by the checkout adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct HighlightToken {
    /// Source text.
    pub(crate) text: String,
    /// Optional renderer class.
    pub(crate) style: Option<String>,
}

/// Unified diff line category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiffLineKind {
    /// Added content.
    Add,
    /// Removed content.
    Remove,
    /// Unchanged context.
    Context,
    /// Hunk header.
    Header,
}

/// One structured diff line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DiffLine {
    /// Line category.
    #[serde(rename = "type")]
    pub(crate) kind: DiffLineKind,
    /// Content without the unified-diff prefix.
    pub(crate) content: String,
    /// Optional syntax-highlight tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tokens: Option<Vec<HighlightToken>>,
}

/// One unified diff hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiffHunk {
    /// First old-file line.
    pub(crate) old_start: u64,
    /// Old-file line count.
    pub(crate) old_count: u64,
    /// First new-file line.
    pub(crate) new_start: u64,
    /// New-file line count.
    pub(crate) new_count: u64,
    /// Header and body lines.
    pub(crate) lines: Vec<DiffLine>,
}

/// Structured diff placeholder/status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ParsedDiffStatus {
    /// The file exceeded a configured diff budget.
    TooLarge,
    /// The file is binary.
    Binary,
}

/// Structured file diff used by live diff and commit history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParsedDiffFile {
    /// Destination path.
    pub(crate) path: String,
    /// Source path for a rename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) old_path: Option<String>,
    /// Whether the destination is new.
    pub(crate) is_new: bool,
    /// Whether the file was deleted.
    pub(crate) is_deleted: bool,
    /// Added line count.
    pub(crate) additions: u64,
    /// Removed line count.
    pub(crate) deletions: u64,
    /// Parsed hunks.
    pub(crate) hunks: Vec<DiffHunk>,
    /// Optional status; omitted for ordinary Paseo-compatible text diffs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<ParsedDiffStatus>,
}

/// One-shot diff result and live-update body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutDiffResult {
    /// Directory from the subscription/request.
    pub(crate) cwd: String,
    /// Path-sorted file diffs.
    pub(crate) files: Vec<ParsedDiffFile>,
    /// Inline error.
    pub(crate) error: Option<CheckoutError>,
    /// True when the total diff was not safe to return.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) diff_too_large: Option<bool>,
}

/// Git commit file status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckoutCommitFileStatus {
    /// Added file.
    Added,
    /// Modified or type-changed file.
    Modified,
    /// Deleted file.
    Deleted,
    /// Renamed file.
    Renamed,
}

/// Per-file commit statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckoutCommitFile {
    /// Destination path.
    pub(crate) path: String,
    /// Added lines; binary files use zero.
    pub(crate) additions: u64,
    /// Removed lines; binary files use zero.
    pub(crate) deletions: u64,
    /// Optional status when Git supplies one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<CheckoutCommitFileStatus>,
}

/// One checkout history commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutCommit {
    /// Full object identity.
    pub(crate) sha: String,
    /// Abbreviated object identity.
    pub(crate) short_sha: String,
    /// First-line subject.
    pub(crate) subject: String,
    /// Git author display name.
    pub(crate) author_name: String,
    /// ISO 8601 author timestamp.
    pub(crate) author_date: String,
    /// False when reachable from no remote ref.
    pub(crate) is_on_remote: bool,
    /// True for bounded base-history context.
    pub(crate) is_on_base: bool,
    /// Changed files.
    pub(crate) files: Vec<CheckoutCommitFile>,
}

/// Checkout commit-list result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutCommitsListResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Resolved comparison ref.
    pub(crate) base_ref: Option<String>,
    /// Workspace commits followed by up to ten base-context commits.
    pub(crate) commits: Vec<CheckoutCommit>,
    /// Inline error.
    pub(crate) error: Option<CheckoutError>,
}

/// Single-file commit diff request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutCommitFileDiffRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Commit-ish to inspect.
    pub(crate) sha: String,
    /// Safe repository-relative path.
    pub(crate) path: String,
}

/// Single-file commit diff result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckoutCommitFileDiffResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Echoed commit-ish.
    pub(crate) sha: String,
    /// Echoed relative path.
    pub(crate) path: String,
    /// Textual diff, or null for missing/binary content.
    pub(crate) file: Option<ParsedDiffFile>,
    /// Inline error.
    pub(crate) error: Option<CheckoutError>,
}

/// Branch existence request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchValidateRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Local name or origin-qualified name to resolve.
    pub(crate) branch_name: String,
}

/// Branch existence result. Paseo uses a plain string error for this query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchValidateResult {
    /// Whether the branch exists locally or on origin.
    pub(crate) exists: bool,
    /// Normalized local branch name.
    pub(crate) resolved_ref: Option<String>,
    /// Whether only the origin tracking ref exists.
    pub(crate) is_remote: bool,
    /// Inline validation or Git error.
    pub(crate) error: Option<String>,
}

/// Branch suggestion request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutBranchSuggestionsRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Optional case-insensitive substring query.
    #[serde(default)]
    pub(crate) query: Option<String>,
    /// Optional result limit in the inclusive range 1..=200.
    #[serde(default)]
    pub(crate) limit: Option<usize>,
}

/// One branch suggestion and its local/origin state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchSuggestion {
    /// Normalized local branch name.
    pub(crate) name: String,
    /// Committer timestamp in Unix seconds.
    pub(crate) committer_date: i64,
    /// Whether a local branch exists.
    pub(crate) has_local: bool,
    /// Whether an origin tracking ref exists.
    pub(crate) has_remote: bool,
    /// Commits present only on the local branch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) local_ahead: Option<u64>,
    /// Commits present only on the origin tracking ref.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) local_behind: Option<u64>,
}

/// Branch suggestions result. Paseo uses a plain string error for this query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchSuggestionsResult {
    /// Ordered branch names retained for compatibility.
    pub(crate) branches: Vec<String>,
    /// Ordered branch details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) branch_details: Option<Vec<CheckoutBranchSuggestion>>,
    /// Inline Git error.
    pub(crate) error: Option<String>,
}

/// Existing-branch checkout request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutBranchSwitchRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Existing local or origin branch.
    pub(crate) branch: String,
}

/// Existing-branch checkout source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CheckoutBranchSource {
    /// Existing local branch.
    Local,
    /// Origin-only branch materialized locally.
    Remote,
}

/// Existing-branch checkout result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchSwitchResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Whether checkout completed.
    pub(crate) success: bool,
    /// Echoed requested branch.
    pub(crate) branch: String,
    /// Resolution source on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<CheckoutBranchSource>,
    /// Inline checkout error.
    pub(crate) error: Option<CheckoutError>,
}

/// Current-branch rename request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutBranchRenameRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// New local branch name.
    pub(crate) branch: String,
}

/// Current-branch rename result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutBranchRenameResult {
    /// Whether rename completed.
    pub(crate) success: bool,
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Renamed branch on success.
    pub(crate) current_branch: Option<String>,
    /// Inline checkout error.
    pub(crate) error: Option<CheckoutError>,
}

/// Commit request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutCommitRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Explicit commit message.
    #[serde(default)]
    pub(crate) message: Option<String>,
    /// Whether all changes should be staged first. Defaults to true.
    #[serde(default)]
    pub(crate) add_all: Option<bool>,
}

/// Merge strategy for merging the current branch into its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CheckoutMergeStrategy {
    /// Ordinary Git merge.
    Merge,
    /// Squash and commit the resulting tree.
    Squash,
}

/// Merge-current-branch-to-base request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutMergeRequest {
    /// Directory inside the current feature checkout.
    pub(crate) cwd: String,
    /// Optional base branch override.
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    /// Merge strategy. Defaults to merge.
    #[serde(default)]
    pub(crate) strategy: Option<CheckoutMergeStrategy>,
    /// Require the request checkout to be clean before operating.
    #[serde(default)]
    pub(crate) require_clean_target: Option<bool>,
}

/// Merge-base-into-current-branch request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutMergeFromBaseRequest {
    /// Directory inside the current feature checkout.
    pub(crate) cwd: String,
    /// Optional base branch override.
    #[serde(default)]
    pub(crate) base_ref: Option<String>,
    /// Require a clean current checkout. Defaults to true.
    #[serde(default)]
    pub(crate) require_clean_target: Option<bool>,
}

/// Managed-workspace reset request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutResetWorkspaceRequest {
    /// Directory inside the managed worktree.
    pub(crate) cwd: String,
    /// Durable workspace identity.
    pub(crate) workspace_id: String,
    /// Branch name saved when the workspace was created.
    pub(crate) initial_branch: String,
}

/// Path-scoped discard request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutDiscardChangesRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Literal repository-relative paths to restore/remove.
    pub(crate) paths: Vec<String>,
}

/// Paseo stash save request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct CheckoutStashSaveRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Optional branch label embedded in the stash message.
    #[serde(default)]
    pub(crate) branch: Option<String>,
}

/// Paseo stash pop request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutStashPopRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Zero-based stash index.
    pub(crate) stash_index: usize,
}

/// Paseo stash list request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutStashListRequest {
    /// Directory inside the checkout.
    pub(crate) cwd: String,
    /// Return only Ait-created stashes; wire name retained for compatibility. Defaults to true.
    #[serde(default)]
    pub(crate) paseo_only: Option<bool>,
}

/// One stash entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckoutStashEntry {
    /// Zero-based stash index.
    pub(crate) index: usize,
    /// Full Git stash subject.
    pub(crate) message: String,
    /// Ait auto-stash branch label, when present.
    pub(crate) branch: Option<String>,
    /// Whether the stash uses the Ait prefix; wire name retained for compatibility.
    pub(crate) is_paseo: bool,
}

/// Stash list result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckoutStashListResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Filtered stash entries.
    pub(crate) entries: Vec<CheckoutStashEntry>,
    /// Inline checkout error.
    pub(crate) error: Option<CheckoutError>,
}

/// Shared result shape for checkout mutations without extra result fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckoutMutationResult {
    /// Echoed request directory.
    pub(crate) cwd: String,
    /// Whether the mutation completed.
    pub(crate) success: bool,
    /// Inline checkout error.
    pub(crate) error: Option<CheckoutError>,
}

#[cfg(test)]
mod tests;
