//! Forge search, pull request, timeline, and check-detail payloads.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable forge availability state copied from Paseo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ForgeAuthState {
    /// The forge CLI is installed and authenticated.
    Authenticated,
    /// The CLI is installed but has no usable credentials.
    Unauthenticated,
    /// The forge CLI is absent.
    CliMissing,
    /// The checkout has no supported forge remote.
    NoRemote,
    /// A non-authentication forge read failed.
    Error,
}

/// Search categories, including Paseo's temporary GitHub aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub(crate) enum ForgeSearchKind {
    /// Forge-neutral issue.
    #[serde(rename = "issue")]
    Issue,
    /// Forge-neutral pull or merge request.
    #[serde(rename = "change_request")]
    ChangeRequest,
    /// Legacy GitHub issue spelling.
    #[serde(rename = "github-issue")]
    GithubIssue,
    /// Legacy GitHub pull request spelling.
    #[serde(rename = "github-pr")]
    GithubPr,
    /// Legacy short pull request spelling.
    #[serde(rename = "pr")]
    Pr,
}

/// Forge or compatibility GitHub search request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForgeSearchRequest {
    /// Checkout used to resolve the forge and repository.
    pub(crate) cwd: String,
    /// Forge search query.
    pub(crate) query: String,
    /// Optional result limit, from one through fifty.
    pub(crate) limit: Option<usize>,
    /// Optional result categories.
    pub(crate) kinds: Option<Vec<ForgeSearchKind>>,
}

/// One forge search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForgeSearchItem {
    /// `issue`, `change_request`, or the legacy `pr` projection.
    pub(crate) kind: String,
    /// Resolved forge brand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) forge: Option<String>,
    /// Forge-local issue or change-request number.
    pub(crate) number: u64,
    /// Display title.
    pub(crate) title: String,
    /// Browser URL.
    pub(crate) url: String,
    /// Open forge state.
    pub(crate) state: String,
    /// Optional body.
    pub(crate) body: Option<String>,
    /// Label names.
    pub(crate) labels: Vec<String>,
    /// Full repository path when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_path: Option<String>,
    /// Base branch for change requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) base_ref_name: Option<String>,
    /// Head branch for change requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) head_ref_name: Option<String>,
    /// Forge timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<String>,
}

/// Neutral forge search response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForgeSearchResult {
    /// Matching issues and change requests.
    pub(crate) items: Vec<ForgeSearchItem>,
    /// Forge availability when it is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_state: Option<ForgeAuthState>,
    /// Non-authentication failure text.
    pub(crate) error: Option<String>,
}

/// GitHub compatibility search response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubSearchResult {
    /// Matching issues and pull requests.
    pub(crate) items: Vec<ForgeSearchItem>,
    /// Legacy availability flag.
    pub(crate) features_enabled: bool,
    /// Forge availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_state: Option<ForgeAuthState>,
    /// Older legacy availability flag.
    pub(crate) github_features_enabled: bool,
    /// Non-authentication failure text.
    pub(crate) error: Option<String>,
}

/// Create a pull request for the current branch.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestCreateRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
    /// Optional explicit title.
    pub(crate) title: Option<String>,
    /// Optional explicit body.
    pub(crate) body: Option<String>,
    /// Optional base branch or ref.
    pub(crate) base_ref: Option<String>,
}

/// Pull request merge method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PullRequestMergeMethod {
    /// Create a merge commit.
    Merge,
    /// Squash the pull request.
    Squash,
    /// Rebase the pull request.
    Rebase,
}

/// Merge the pull request associated with the current branch.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestMergeRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
    /// Requested forge merge method.
    pub(crate) merge_method: PullRequestMergeMethod,
}

/// Enable or disable auto-merge for the current pull request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestAutoMergeRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
    /// Desired auto-merge state.
    pub(crate) enabled: bool,
    /// Required when enabling and forbidden when disabling.
    pub(crate) merge_method: Option<PullRequestMergeMethod>,
}

/// A checkout-scoped forge read request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct ForgePathRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
}

/// Pull request timeline request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestTimelineRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
    /// Pull request number.
    pub(crate) pr_number: u64,
    /// GitHub repository owner.
    pub(crate) repo_owner: String,
    /// GitHub repository name.
    pub(crate) repo_name: String,
}

/// Detailed check request.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckDetailsRequest {
    /// Checkout directory.
    pub(crate) cwd: String,
    /// GitHub repository owner.
    pub(crate) repo_owner: Option<String>,
    /// GitHub repository name.
    pub(crate) repo_name: Option<String>,
    /// GitHub check-run identifier.
    pub(crate) check_run_id: Option<u64>,
    /// GitHub Actions workflow-run identifier.
    pub(crate) workflow_run_id: Option<u64>,
    /// Change request number used by other forge families.
    pub(crate) change_request_number: Option<u64>,
}

/// Pull request creation response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PullRequestCreateResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Browser URL on success.
    pub(crate) url: Option<String>,
    /// Pull request number on success.
    pub(crate) number: Option<u64>,
    /// Inline checkout-shaped error.
    pub(crate) error: Option<crate::git::protocol::checkout::CheckoutError>,
}

/// Generic pull request mutation response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PullRequestMutationResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Whether the operation completed.
    pub(crate) success: bool,
    /// Inline checkout-shaped error.
    pub(crate) error: Option<crate::git::protocol::checkout::CheckoutError>,
}

/// Auto-merge mutation response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PullRequestAutoMergeResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Requested state.
    pub(crate) enabled: bool,
    /// Whether the operation completed.
    pub(crate) success: bool,
    /// Inline checkout-shaped error.
    pub(crate) error: Option<crate::git::protocol::checkout::CheckoutError>,
}

/// Pull request mergeability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum PullRequestMergeable {
    /// Forge reports the request can merge.
    Mergeable,
    /// Forge reports conflicts.
    Conflicting,
    /// Forge does not know yet.
    Unknown,
}

/// One normalized CI check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestCheck {
    /// Check name.
    pub(crate) name: String,
    /// Normalized lifecycle.
    pub(crate) status: String,
    /// Details URL.
    pub(crate) url: Option<String>,
    /// Workflow display name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workflow: Option<String>,
    /// Formatted run duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) duration: Option<String>,
    /// Check-run identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) check_run_id: Option<u64>,
    /// Workflow-run identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workflow_run_id: Option<u64>,
    /// Open forge-neutral refinements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) traits: Option<Vec<String>>,
}

/// Current pull request status.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestStatus {
    /// Resolved forge brand.
    pub(crate) forge: String,
    /// Full forge project path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) project_path: Option<String>,
    /// Pull request number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) number: Option<u64>,
    /// Browser URL.
    pub(crate) url: String,
    /// Display title.
    pub(crate) title: String,
    /// Forge state.
    pub(crate) state: String,
    /// Base branch.
    pub(crate) base_ref_name: String,
    /// Head branch.
    pub(crate) head_ref_name: String,
    /// Source commit recorded by this request; absent for older forge adapters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) head_sha: Option<String>,
    /// Whether the request is merged.
    pub(crate) is_merged: bool,
    /// Whether it is a draft.
    pub(crate) is_draft: bool,
    /// Forge mergeability.
    pub(crate) mergeable: PullRequestMergeable,
    /// Normalized checks.
    pub(crate) checks: Vec<PullRequestCheck>,
    /// Aggregate check state.
    pub(crate) checks_status: String,
    /// Review decision.
    pub(crate) review_decision: Option<String>,
    /// Repository owner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) repo_owner: Option<String>,
    /// Repository name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) repo_name: Option<String>,
    /// Legacy GitHub facts mirror.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) github: Option<Value>,
    /// Open forge-specific facts envelope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) forge_specific: Option<Value>,
}

/// Current pull request status response.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestStatusResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Current pull request, or null.
    pub(crate) status: Option<PullRequestStatus>,
    /// Paseo compatibility availability flag.
    pub(crate) github_features_enabled: bool,
    /// Forge availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_state: Option<ForgeAuthState>,
    /// Resolved forge brand.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) forge: Option<String>,
    /// Inline checkout-shaped error.
    pub(crate) error: Option<crate::git::protocol::checkout::CheckoutError>,
}

/// Pull request timeline review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimelineReviewState {
    /// Approved review.
    Approved,
    /// Changes requested.
    ChangesRequested,
    /// General review comment.
    Commented,
}

/// Optional file position for a timeline comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimelineCommentLocation {
    /// Repository-relative path.
    pub(crate) path: String,
    /// Ending line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<u64>,
    /// Starting line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_line: Option<u64>,
    /// Forge thread identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) thread_id: Option<String>,
    /// Resolution state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_resolved: Option<bool>,
    /// Whether the location is stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) is_outdated: Option<bool>,
}

/// Timeline review or comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PullRequestTimelineItem {
    /// Pull request review.
    Review {
        /// Forge node identifier.
        id: String,
        /// Author login.
        author: String,
        /// Author profile URL.
        #[serde(rename = "authorUrl")]
        author_url: Option<String>,
        /// Author avatar URL.
        #[serde(rename = "avatarUrl")]
        avatar_url: Option<String>,
        /// Markdown body.
        body: String,
        /// Unix epoch milliseconds.
        #[serde(rename = "createdAt")]
        created_at: i64,
        /// Browser URL.
        url: String,
        /// Normalized review decision.
        #[serde(rename = "reviewState")]
        review_state: TimelineReviewState,
    },
    /// General or inline comment.
    Comment {
        /// Forge node identifier.
        id: String,
        /// Author login.
        author: String,
        /// Author profile URL.
        #[serde(rename = "authorUrl")]
        author_url: Option<String>,
        /// Author avatar URL.
        #[serde(rename = "avatarUrl")]
        avatar_url: Option<String>,
        /// Markdown body.
        body: String,
        /// Unix epoch milliseconds.
        #[serde(rename = "createdAt")]
        created_at: i64,
        /// Browser URL.
        url: String,
        /// Parent review identifier.
        #[serde(rename = "reviewId", skip_serializing_if = "Option::is_none")]
        review_id: Option<String>,
        /// Thread identifier independent of a file location.
        #[serde(rename = "threadId", skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        /// General-thread resolution state.
        #[serde(rename = "threadIsResolved", skip_serializing_if = "Option::is_none")]
        thread_is_resolved: Option<bool>,
        /// Optional inline position.
        #[serde(skip_serializing_if = "Option::is_none")]
        location: Option<TimelineCommentLocation>,
    },
}

/// Pull request timeline error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimelineErrorKind {
    /// Pull request not found.
    NotFound,
    /// Caller lacks permission.
    Forbidden,
    /// Another failure occurred.
    Unknown,
}

/// Inline timeline error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TimelineError {
    /// Stable category.
    pub(crate) kind: TimelineErrorKind,
    /// Local diagnostic.
    pub(crate) message: String,
}

/// Pull request timeline response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullRequestTimelineResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Pull request number.
    pub(crate) pr_number: Option<u64>,
    /// Stable timeline items.
    pub(crate) items: Vec<PullRequestTimelineItem>,
    /// Whether a forge page limit truncated the timeline.
    pub(crate) truncated: bool,
    /// Inline timeline error.
    pub(crate) error: Option<TimelineError>,
    /// Paseo compatibility availability flag.
    pub(crate) github_features_enabled: bool,
    /// Forge availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_state: Option<ForgeAuthState>,
}

/// Check-run annotation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckAnnotation {
    /// Repository-relative path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    /// First annotated line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_line: Option<u64>,
    /// Last annotated line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) end_line: Option<u64>,
    /// Forge annotation severity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) annotation_level: Option<String>,
    /// Main message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message: Option<String>,
    /// Annotation title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Additional details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) raw_details: Option<String>,
}

/// Failed workflow job summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckFailedJob {
    /// Job identifier.
    pub(crate) job_id: u64,
    /// Job name.
    pub(crate) name: String,
    /// Forge job state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    /// Forge conclusion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conclusion: Option<String>,
    /// Browser URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    /// Bounded log tail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) log_tail: Option<String>,
    /// Whether the log was truncated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) log_truncated: Option<bool>,
}

/// GitHub check output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct CheckOutput {
    /// Output title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Markdown summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) summary: Option<String>,
    /// Markdown details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
}

/// Forge check details. `pipeline` remains an open envelope for non-GitHub adapters.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CheckDetails {
    /// Check-run identifier.
    pub(crate) check_run_id: u64,
    /// Workflow-run identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) workflow_run_id: Option<u64>,
    /// Check name.
    pub(crate) name: String,
    /// Forge state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    /// Forge conclusion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) conclusion: Option<String>,
    /// Browser URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
    /// Additional details URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) details_url: Option<String>,
    /// Check output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output: Option<CheckOutput>,
    /// Check annotations.
    pub(crate) annotations: Vec<CheckAnnotation>,
    /// Failed jobs in the workflow.
    pub(crate) failed_jobs: Vec<CheckFailedJob>,
    /// Whether annotations, jobs, or logs were truncated.
    pub(crate) truncated: bool,
    /// Structured pipeline for pipeline-oriented forges.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pipeline: Option<Value>,
}

/// Check-details response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct CheckDetailsResult {
    /// Echoed checkout directory.
    pub(crate) cwd: String,
    /// Whether the read completed.
    pub(crate) success: bool,
    /// Details on success.
    pub(crate) details: Option<CheckDetails>,
    /// Inline checkout-shaped error.
    pub(crate) error: Option<crate::git::protocol::checkout::CheckoutError>,
}

#[cfg(test)]
mod tests;
