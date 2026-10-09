//! Blocking forge adapter contract.

use serde_json::Value;

/// Forge failure category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForgeFailureKind {
    /// Directory is outside a Git repository.
    NotGitRepository,
    /// Input or path is not allowed.
    NotAllowed,
    /// Pull request has merge conflicts.
    MergeConflict,
    /// Required forge CLI is absent.
    CliMissing,
    /// Forge CLI is not authenticated.
    Unauthenticated,
    /// No supported forge remote is configured.
    NoRemote,
    /// Forge object was not found.
    NotFound,
    /// Forge denied access.
    Forbidden,
    /// Request parameters are invalid.
    Invalid,
    /// Command, network, parsing, or another forge failure.
    Unknown,
}

/// Forge adapter failure with a local diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ForgeRuntimeError {
    /// Resolved forge when the failure happened after platform detection.
    pub(crate) forge: Option<String>,
    /// Stable failure category.
    pub(crate) kind: ForgeFailureKind,
    /// Human-readable diagnostic.
    pub(crate) message: String,
}

/// Forge availability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForgeAuthState {
    /// CLI is available and authenticated.
    Authenticated,
    /// CLI has no usable credentials.
    Unauthenticated,
    /// CLI executable is absent.
    CliMissing,
    /// Checkout has no supported forge remote.
    NoRemote,
    /// A non-authentication forge read failed.
    Error,
}

/// Normalized forge search category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeSearchKind {
    /// Issue.
    Issue,
    /// Pull or merge request.
    ChangeRequest,
}

/// One forge search item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForgeSearchItem {
    /// Result category.
    pub(crate) kind: ForgeSearchKind,
    /// Forge brand when resolved.
    pub(crate) forge: Option<String>,
    /// Forge-local number.
    pub(crate) number: u64,
    /// Title.
    pub(crate) title: String,
    /// Browser URL.
    pub(crate) url: String,
    /// Open forge state.
    pub(crate) state: String,
    /// Body.
    pub(crate) body: Option<String>,
    /// Label names.
    pub(crate) labels: Vec<String>,
    /// Full project path.
    pub(crate) project_path: Option<String>,
    /// Base branch for change requests.
    pub(crate) base_ref_name: Option<String>,
    /// Head branch for change requests.
    pub(crate) head_ref_name: Option<String>,
    /// Forge timestamp.
    pub(crate) updated_at: Option<String>,
}

/// Forge search outcome, including unavailable results that are not request failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeSearch {
    /// Search results.
    pub(crate) items: Vec<ForgeSearchItem>,
    /// Availability state.
    pub(crate) auth_state: ForgeAuthState,
}

/// Pull request merge method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestMergeMethod {
    /// Merge commit.
    Merge,
    /// Squash merge.
    Squash,
    /// Rebase merge.
    Rebase,
}

/// Created pull request identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestCreated {
    /// Browser URL.
    pub(crate) url: String,
    /// Pull request number.
    pub(crate) number: u64,
}

/// Pull request mergeability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PullRequestMergeable {
    /// Mergeable.
    Mergeable,
    /// Conflicting.
    Conflicting,
    /// Unknown.
    Unknown,
}

/// One normalized pull request check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PullRequestCheck {
    /// Check name.
    pub(crate) name: String,
    /// Normalized lifecycle.
    pub(crate) status: String,
    /// Details URL.
    pub(crate) url: Option<String>,
    /// Workflow display name.
    pub(crate) workflow: Option<String>,
    /// Formatted duration.
    pub(crate) duration: Option<String>,
    /// Check-run identifier.
    pub(crate) check_run_id: Option<u64>,
    /// Workflow-run identifier.
    pub(crate) workflow_run_id: Option<u64>,
    /// Open refinements.
    pub(crate) traits: Option<Vec<String>>,
}

/// Current pull request status.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PullRequestStatus {
    /// Forge brand.
    pub(crate) forge: String,
    /// Full project path.
    pub(crate) project_path: Option<String>,
    /// Pull request number.
    pub(crate) number: Option<u64>,
    /// Browser URL.
    pub(crate) url: String,
    /// Title.
    pub(crate) title: String,
    /// Forge state.
    pub(crate) state: String,
    /// Base branch.
    pub(crate) base_ref_name: String,
    /// Head branch.
    pub(crate) head_ref_name: String,
    /// Source commit recorded by this pull request, including after closure.
    pub(crate) head_sha: Option<String>,
    /// Merged state.
    pub(crate) is_merged: bool,
    /// Draft state.
    pub(crate) is_draft: bool,
    /// Mergeability.
    pub(crate) mergeable: PullRequestMergeable,
    /// Checks.
    pub(crate) checks: Vec<PullRequestCheck>,
    /// Aggregate check state.
    pub(crate) checks_status: String,
    /// Review decision.
    pub(crate) review_decision: Option<String>,
    /// Repository owner.
    pub(crate) repo_owner: Option<String>,
    /// Repository name.
    pub(crate) repo_name: Option<String>,
    /// Legacy GitHub facts mirror.
    pub(crate) github: Option<Value>,
    /// Forge-specific facts.
    pub(crate) forge_specific: Option<Value>,
}

/// Current pull request read, including forge availability.
#[derive(Debug, Clone, PartialEq)]
pub struct PullRequestStatusRead {
    /// Current pull request, or none.
    pub(crate) status: Option<PullRequestStatus>,
    /// Availability state.
    pub(crate) auth_state: ForgeAuthState,
    /// Resolved forge brand.
    pub(crate) forge: Option<String>,
}

/// Timeline review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimelineReviewState {
    /// Approved review.
    Approved,
    /// Changes requested.
    ChangesRequested,
    /// General review comment.
    Commented,
}

/// Optional inline comment location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineCommentLocation {
    /// Repository-relative path.
    pub(crate) path: String,
    /// Ending line.
    pub(crate) line: Option<u64>,
    /// Starting line.
    pub(crate) start_line: Option<u64>,
    /// Thread identifier.
    pub(crate) thread_id: Option<String>,
    /// Resolution state.
    pub(crate) is_resolved: Option<bool>,
    /// Outdated state.
    pub(crate) is_outdated: Option<bool>,
}

/// Pull request review or comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PullRequestTimelineItem {
    /// Review.
    Review {
        /// Node identifier.
        id: String,
        /// Author login.
        author: String,
        /// Author profile URL.
        author_url: Option<String>,
        /// Author avatar URL.
        avatar_url: Option<String>,
        /// Markdown body.
        body: String,
        /// Unix epoch milliseconds.
        created_at: i64,
        /// Browser URL.
        url: String,
        /// Review decision.
        review_state: TimelineReviewState,
    },
    /// Comment.
    Comment {
        /// Node identifier.
        id: String,
        /// Author login.
        author: String,
        /// Author profile URL.
        author_url: Option<String>,
        /// Author avatar URL.
        avatar_url: Option<String>,
        /// Markdown body.
        body: String,
        /// Unix epoch milliseconds.
        created_at: i64,
        /// Browser URL.
        url: String,
        /// Parent review identifier.
        review_id: Option<String>,
        /// General thread identifier.
        thread_id: Option<String>,
        /// General thread resolution state.
        thread_is_resolved: Option<bool>,
        /// Inline location.
        location: Option<TimelineCommentLocation>,
    },
}

/// Timeline error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimelineErrorKind {
    /// Pull request not found.
    NotFound,
    /// Access forbidden.
    Forbidden,
}

/// Inline timeline error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimelineError {
    /// Stable category.
    pub(crate) kind: TimelineErrorKind,
    /// Local diagnostic.
    pub(crate) message: String,
}

/// Pull request timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestTimeline {
    /// Pull request number.
    pub(crate) pr_number: u64,
    /// Stable items.
    pub(crate) items: Vec<PullRequestTimelineItem>,
    /// Page truncation.
    pub(crate) truncated: bool,
    /// Inline forge error.
    pub(crate) error: Option<TimelineError>,
    /// Forge availability.
    pub(crate) auth_state: ForgeAuthState,
}

/// Check annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckAnnotation {
    /// Repository-relative path.
    pub(crate) path: Option<String>,
    /// First line.
    pub(crate) start_line: Option<u64>,
    /// Last line.
    pub(crate) end_line: Option<u64>,
    /// Severity.
    pub(crate) annotation_level: Option<String>,
    /// Message.
    pub(crate) message: Option<String>,
    /// Title.
    pub(crate) title: Option<String>,
    /// Additional details.
    pub(crate) raw_details: Option<String>,
}

/// Failed workflow job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckFailedJob {
    /// Job identifier.
    pub(crate) job_id: u64,
    /// Job name.
    pub(crate) name: String,
    /// Forge state.
    pub(crate) status: Option<String>,
    /// Forge conclusion.
    pub(crate) conclusion: Option<String>,
    /// Browser URL.
    pub(crate) url: Option<String>,
    /// Bounded log tail.
    pub(crate) log_tail: Option<String>,
    /// Log truncation.
    pub(crate) log_truncated: Option<bool>,
}

/// Check output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CheckOutput {
    /// Title.
    pub(crate) title: Option<String>,
    /// Markdown summary.
    pub(crate) summary: Option<String>,
    /// Markdown detail.
    pub(crate) text: Option<String>,
}

/// Detailed check result.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckDetails {
    /// Check-run identifier.
    pub(crate) check_run_id: u64,
    /// Workflow-run identifier.
    pub(crate) workflow_run_id: Option<u64>,
    /// Check name.
    pub(crate) name: String,
    /// Forge state.
    pub(crate) status: Option<String>,
    /// Forge conclusion.
    pub(crate) conclusion: Option<String>,
    /// Browser URL.
    pub(crate) url: Option<String>,
    /// Details URL.
    pub(crate) details_url: Option<String>,
    /// Output.
    pub(crate) output: Option<CheckOutput>,
    /// Annotations.
    pub(crate) annotations: Vec<CheckAnnotation>,
    /// Failed jobs.
    pub(crate) failed_jobs: Vec<CheckFailedJob>,
    /// Truncation flag.
    pub(crate) truncated: bool,
    /// Pipeline-oriented forge details.
    pub(crate) pipeline: Option<Value>,
}

/// Blocking forge runtime.
pub trait ForgeRuntime: std::fmt::Debug + Send {
    /// Installed platform implementations; authentication remains per-host.
    fn providers(&self) -> &'static [&'static str] {
        &["github"]
    }

    /// Search issues and change requests.
    ///
    /// # Errors
    /// Returns categorized local Git, CLI, authentication, or forge failures.
    fn search(
        &self,
        cwd: &str,
        query: &str,
        limit: usize,
        kinds: &[ForgeSearchKind],
    ) -> Result<ForgeSearch, ForgeRuntimeError>;

    /// Push the current branch and create a pull request.
    ///
    /// # Errors
    /// Returns categorized local Git, push, CLI, authentication, or forge failures.
    fn create_pull_request(
        &self,
        cwd: &str,
        title: &str,
        body: &str,
        base_ref: Option<&str>,
    ) -> Result<PullRequestCreated, ForgeRuntimeError>;

    /// Read the current branch's pull request.
    ///
    /// # Errors
    /// Returns categorized local Git, CLI, authentication, or forge failures.
    fn current_pull_request_status(
        &self,
        cwd: &str,
    ) -> Result<PullRequestStatusRead, ForgeRuntimeError>;

    /// Merge the current pull request.
    ///
    /// # Errors
    /// Returns resolution, validation, CLI, authentication, or forge failures.
    fn merge_current_pull_request(
        &self,
        cwd: &str,
        merge_method: PullRequestMergeMethod,
    ) -> Result<(), ForgeRuntimeError>;

    /// Enable or disable auto-merge on the current pull request.
    ///
    /// # Errors
    /// Returns resolution, validation, CLI, authentication, or forge failures.
    fn set_current_pull_request_auto_merge(
        &self,
        cwd: &str,
        enabled: bool,
        merge_method: Option<PullRequestMergeMethod>,
    ) -> Result<(), ForgeRuntimeError>;

    /// Read a pull request timeline.
    ///
    /// # Errors
    /// Returns identity, CLI, authentication, or forge failures.
    fn pull_request_timeline(
        &self,
        cwd: &str,
        pr_number: u64,
        repo_owner: &str,
        repo_name: &str,
    ) -> Result<PullRequestTimeline, ForgeRuntimeError>;

    /// Read detailed CI check data.
    ///
    /// # Errors
    /// Returns check identity, CLI, authentication, or forge failures.
    fn check_details(
        &self,
        cwd: &str,
        query: CheckDetailsQuery<'_>,
    ) -> Result<CheckDetails, ForgeRuntimeError>;
}

/// Identity of a CI check lookup within a checkout.
#[derive(Debug, Clone, Copy, Default)]
pub struct CheckDetailsQuery<'a> {
    /// Repository owner when required by the forge.
    pub(crate) repo_owner: Option<&'a str>,
    /// Repository name when required by the forge.
    pub(crate) repo_name: Option<&'a str>,
    /// Individual check run ID.
    pub(crate) check_run_id: Option<u64>,
    /// Workflow run ID.
    pub(crate) workflow_run_id: Option<u64>,
    /// Associated pull or merge request number.
    pub(crate) change_request_number: Option<u64>,
}
