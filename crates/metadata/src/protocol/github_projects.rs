//! Paseo-shaped GitHub repository search and project clone payloads.

use domain::workspace::protocol::workspace::WorkspaceProjectDescriptorPayload;
use serde::{Deserialize, Serialize};

/// Repository discovery input. Empty query lists recent owned repositories.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct GithubRepositorySearchRequest {
    /// Search text, trimmed before invoking GitHub CLI.
    pub(crate) query: String,
    /// Maximum result count, 1–50; defaults to 20.
    pub(crate) limit: Option<usize>,
}

/// GitHub repository visibility supported by the CLI's `isPrivate` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GithubRepositoryVisibility {
    /// Public repository.
    Public,
    /// Private repository.
    Private,
}

/// Normalized GitHub repository in a search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubRepositoryPayload {
    /// GraphQL or numeric GitHub identity as text.
    pub(crate) id: String,
    /// Repository name.
    pub(crate) name: String,
    /// Full owner/repository path.
    pub(crate) name_with_owner: String,
    /// Optional description.
    pub(crate) description: Option<String>,
    /// Public or private visibility.
    pub(crate) visibility: GithubRepositoryVisibility,
    /// GitHub update timestamp.
    pub(crate) updated_at: String,
    /// Clone URL chosen from host GitHub CLI configuration.
    pub(crate) clone_url: String,
}

/// GitHub repository search status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GithubRepositorySearchStatus {
    /// Search completed.
    Success,
    /// GitHub CLI is absent.
    Unavailable,
    /// GitHub CLI needs authentication.
    Unauthenticated,
    /// Another search or parsing failure occurred.
    Error,
}

/// Search result with Paseo's availability and optional missing-CLI reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GithubRepositorySearchResult {
    /// Search status.
    pub(crate) status: GithubRepositorySearchStatus,
    /// Matching repositories; empty for failure states.
    pub(crate) repositories: Vec<GithubRepositoryPayload>,
    /// Whether the CLI was available for this request.
    pub(crate) available: bool,
    /// `gh_missing` only for the unavailable status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<&'static str>,
    /// Null on success, otherwise a safe diagnostic.
    pub(crate) error: Option<String>,
}

/// Clone transport for an owner/repository input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GithubCloneProtocol {
    /// HTTPS remote.
    Https,
    /// SSH remote.
    Ssh,
}

/// Clone a GitHub repository into a new child of the target directory.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectGithubCloneRequest {
    /// Owner/repository pair or supported GitHub clone URL.
    pub(crate) repo: String,
    /// Optional transport for an owner/repository pair.
    pub(crate) clone_protocol: Option<GithubCloneProtocol>,
    /// Parent directory; `~` and relative paths follow host resolution.
    pub(crate) target_directory: String,
}

/// Completed clone and Project registration, or an inline business error.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectGithubCloneResult {
    /// Normalized owner/repository path when valid.
    pub(crate) repo: String,
    /// Completed checkout path, even if later registration fails.
    pub(crate) checkout_path: Option<String>,
    /// Registered Project, if any.
    pub(crate) project: Option<WorkspaceProjectDescriptorPayload>,
    /// Safe business failure or null on success.
    pub(crate) error: Option<String>,
}

#[cfg(test)]
mod tests;
