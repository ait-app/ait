//! Summary artifacts, provider selection, input values and safe generation failures.

use serde::{Deserialize, Serialize};

/// The four independently configurable Paseo summary styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryKind {
    /// A conversation's display title.
    Title,
    /// A workspace title and independently generated Git branch.
    BranchName,
    /// A Git commit subject.
    CommitMessage,
    /// A pull request title and Markdown body.
    PullRequest,
}

/// An ordered provider/model selection without credentials or foreground permissions.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarySelection {
    /// Registered provider identity.
    pub provider: String,
    /// Explicit model, or the provider default.
    pub model: Option<String>,
    /// Provider reasoning option, when selected.
    pub thinking_option_id: Option<String>,
}

/// Owned input to a bounded, non-persisted summary generation operation.
#[derive(Debug, Clone)]
pub struct SummaryRequest {
    /// Artifact to generate and validate.
    pub kind: SummaryKind,
    /// Existing working directory for model discovery and project styles.
    pub cwd: String,
    /// Prompt/attachments or a bounded Git diff, treated as source material.
    pub context: String,
    /// Current foreground selection, tried after configured and automatic candidates.
    pub selection: Option<SummarySelection>,
}

/// Safe summary failures; raw model output and diagnostics never become public errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SummaryError {
    /// No candidate produced valid output within the operation's budget.
    #[error("summary generation unavailable")]
    Unavailable,
    /// A bounded queue is full or the server is shutting down.
    #[error("summary generation busy or cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests;
