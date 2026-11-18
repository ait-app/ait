//! Forge-neutral facts about one pull request (a GitLab merge request): its state, checks and
//! newest review activity, as a forge adapter reads them and Bot tickets follow them (ADR-287).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Most checks one read of a pull request returns; forge adapters bound their reads to it so
/// every forge keeps the same checks (ADR-287).
pub const CHECK_LIMIT: usize = 100;

/// Whether a pull request is still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullState {
    /// Open, including drafts.
    Open,
    /// Merged.
    Merged,
    /// Closed without merging.
    Closed,
}

impl PullState {
    /// Whether the pull request will see no more changes unless reopened.
    #[must_use]
    pub const fn is_ended(self) -> bool {
        matches!(self, Self::Merged | Self::Closed)
    }
}

/// Normalized result of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Queued or running.
    Pending,
    /// Passed.
    Success,
    /// Failed, timed out or needs action.
    Failure,
    /// Skipped, neutral or cancelled.
    Skipped,
}

impl CheckStatus {
    /// The status of a forge adapter's typed check, folding cancelled checks into skipped ones.
    ///
    /// # Arguments
    ///
    /// * `status` - Check status as the forge adapters normalize it for workspace snapshots.
    #[must_use]
    pub const fn from_forge(status: crate::workspace::protocol::workspace::CheckStatus) -> Self {
        use crate::workspace::protocol::workspace::CheckStatus as Forge;
        match status {
            Forge::Success => Self::Success,
            Forge::Failure => Self::Failure,
            Forge::Skipped | Forge::Cancelled => Self::Skipped,
            Forge::Pending => Self::Pending,
        }
    }
}

/// One check of the pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullCheck {
    /// Check name, unique within the pull request.
    pub name: String,
    /// Result.
    pub status: CheckStatus,
    /// Details URL.
    pub url: Option<String>,
    /// Forge run identity (check run or job id), when the forge has one.
    pub run: Option<String>,
}

/// Decision of a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    /// Approved.
    Approved,
    /// Changes requested.
    ChangesRequested,
    /// Commented without a decision.
    Commented,
}

/// What a timeline entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullItemKind {
    /// A comment on the pull request conversation.
    Comment,
    /// A comment on a line of the diff.
    ReviewComment,
    /// A submitted review.
    Review(ReviewState),
}

/// What the author of a timeline entry may do in the repository, as the forge reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorAccess {
    /// Can write to the repository: GitHub owner, or an author whose permission lookup
    /// confirmed write access; GitLab Developer or above.
    Writer,
    /// A GitHub organization member or collaborator, who may hold only read or triage access:
    /// a writer only once a permission lookup confirms it (ADR-287).
    Unconfirmed,
    /// Anyone else, such as a stranger on a public repository.
    Other,
    /// A bot or app account.
    Bot,
}

/// One review, review comment or comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullItem {
    /// Forge node identity, stable across reads.
    pub id: String,
    /// Kind.
    pub kind: PullItemKind,
    /// Author login.
    pub author: String,
    /// Written by the account the forge CLI is signed in as.
    pub own: bool,
    /// What the author may do in the repository.
    pub access: AuthorAccess,
    /// Markdown body.
    pub body: String,
    /// Browser URL.
    pub url: String,
    /// When it was written.
    pub created_at: DateTime<Utc>,
    /// File of an inline comment.
    pub path: Option<String>,
    /// Line of an inline comment.
    pub line: Option<u64>,
}

/// One read of a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullSnapshot {
    /// Number within its repository.
    pub number: u64,
    /// Browser URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// State.
    pub state: PullState,
    /// Head commit.
    pub head: Option<String>,
    /// Checks of the head commit.
    pub checks: Vec<PullCheck>,
    /// Newest timeline entries, in no particular order.
    pub items: Vec<PullItem>,
}

#[cfg(test)]
mod tests;
