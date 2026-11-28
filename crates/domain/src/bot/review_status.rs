//! The status a GitHub review channel keeps in one comment per pull request (ADR-321).
//!
//! Lines are only ever appended: a state change adds one, a newer head marks the earlier heads
//! outdated. Strikethrough is derived when rendering, never stored. These records are new, so
//! they accept unknown fields.

use serde::{Deserialize, Serialize};

use crate::ChannelId;
use crate::bot::GithubReviewTrigger;

/// Longest kept failure reason in bytes.
const REASON_LIMIT: usize = 512;

/// Which pull request a status belongs to.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStatusKey {
    /// Channel whose App posts the status.
    pub channel_id: ChannelId,
    /// Canonical lowercase `owner/name`.
    pub repository: String,
    /// Positive pull-request number.
    pub pull_number: u64,
}

/// The durable status board of one pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStatus {
    /// The pull request.
    pub key: ReviewStatusKey,
    /// GitHub comments that hold the pages, in order.
    #[serde(default)]
    pub pages: Vec<ReviewStatusPage>,
    /// Bumped by every change.
    pub revision: u64,
    /// Last revision written to GitHub.
    #[serde(default)]
    pub published_revision: u64,
    /// Trigger comments still waiting for an eyes reaction.
    #[serde(default)]
    pub pending_reactions: Vec<u64>,
    /// Reviewed heads, oldest first.
    #[serde(default)]
    pub heads: Vec<ReviewHead>,
}

/// The last body sent for a status comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStatusPage {
    /// GitHub issue comment ID.
    pub comment_id: u64,
    /// SHA-256 of the rendered Markdown.
    pub body_hash: String,
}

/// The lines of one reviewed head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHead {
    /// Full commit object name.
    pub head_sha: String,
    /// Lines in the order they happened.
    pub lines: Vec<StatusLine>,
}

/// One appended line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusLine {
    /// Append order across all heads on this pull request.
    pub sequence: u64,
    /// RFC 3339 time the event happened.
    pub at: String,
    /// What happened.
    pub event: StatusEvent,
}

/// What a line reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum StatusEvent {
    /// A permitted user asked for a review of this head.
    Requested {
        /// GitHub login.
        user: String,
        /// How the review was asked for.
        trigger: GithubReviewTrigger,
    },
    /// The same head was asked for again; the running review covers it.
    RequestedAgain {
        /// GitHub login.
        user: String,
        /// How the review was asked for.
        trigger: GithubReviewTrigger,
        /// Trigger comment, when this request was made in a comment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        comment_id: Option<u64>,
    },
    /// The review is waiting for a worker.
    Queued,
    /// The frozen checkout is being prepared.
    Preparing,
    /// The model is reviewing.
    Reviewing {
        /// Provider identity, such as `codex`.
        provider: String,
    },
    /// GitHub rate-limited publishing; it is retried after the wait.
    RateLimited {
        /// Seconds until the next attempt.
        wait_seconds: u64,
    },
    /// The review was published.
    Published {
        /// Findings by severity.
        high: u64,
        /// Findings by severity.
        medium: u64,
        /// Findings by severity.
        low: u64,
        /// Link to the published review, when GitHub returned one.
        url: Option<String>,
    },
    /// The review could not run.
    Failed {
        /// Safe reason shown on the pull request.
        reason: String,
    },
    /// Whether the review ran or was published is unknown.
    Unknown,
    /// The review was cancelled.
    Cancelled,
    /// Newer commits were pushed; this head's result is outdated.
    Outdated {
        /// Full commit object name of the newer head.
        head_sha: String,
    },
}

impl ReviewStatus {
    /// An empty board for `key`.
    #[must_use]
    pub const fn new(key: ReviewStatusKey) -> Self {
        Self {
            key,
            pages: Vec::new(),
            revision: 0,
            published_revision: 0,
            pending_reactions: Vec::new(),
            heads: Vec::new(),
        }
    }

    /// Whether GitHub has yet to see the latest change.
    #[must_use]
    pub const fn dirty(&self) -> bool {
        self.revision > self.published_revision || !self.pending_reactions.is_empty()
    }

    /// Append `event` to `head` at `at`. A newly requested head marks earlier reviews outdated.
    pub fn append(&mut self, head: &str, event: StatusEvent, at: &str) {
        let index = if let Some(index) = self.heads.iter().position(|known| known.head_sha == head)
        {
            index
        } else {
            self.observe_head(head, at);
            self.heads.push(ReviewHead {
                head_sha: head.to_owned(),
                lines: Vec::new(),
            });
            self.heads.len() - 1
        };
        let event = match event {
            StatusEvent::Failed { reason } => StatusEvent::Failed {
                reason: bounded(&reason),
            },
            event => event,
        };
        self.add_line(index, event, at);
    }

    /// Mark the previous review outdated when GitHub reports a different `head`.
    pub fn observe_head(&mut self, head: &str, at: &str) {
        let Some(previous) = self.heads.last() else {
            return;
        };
        if previous.head_sha == head
            || previous
                .lines
                .last()
                .is_some_and(|line| matches!(line.event, StatusEvent::Outdated { .. }))
        {
            return;
        }
        self.add_line(
            self.heads.len() - 1,
            StatusEvent::Outdated {
                head_sha: head.to_owned(),
            },
            at,
        );
    }

    /// Ask for an eyes reaction on trigger comment `comment`.
    pub fn react(&mut self, comment: u64) {
        if !self.pending_reactions.contains(&comment) {
            self.pending_reactions.push(comment);
        }
    }

    fn add_line(&mut self, index: usize, event: StatusEvent, at: &str) {
        self.revision += 1;
        let lines = &mut self.heads[index].lines;
        let position = match lines.last() {
            Some(last) if matches!(last.event, StatusEvent::Outdated { .. }) => lines.len() - 1,
            _ => lines.len(),
        };
        lines.insert(
            position,
            StatusLine {
                sequence: self.revision,
                at: at.to_owned(),
                event,
            },
        );
    }
}

fn bounded(reason: &str) -> String {
    let reason = reason.replace(['\n', '\r'], " ");
    let end = reason.floor_char_boundary(reason.len().min(REASON_LIMIT));
    reason[..end].to_owned()
}

#[cfg(test)]
mod tests;
