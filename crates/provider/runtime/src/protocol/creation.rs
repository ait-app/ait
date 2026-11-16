//! Optional Git placement fields accepted by Agent creation.

use serde::Deserialize;

/// Explicit managed worktree selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub(crate) enum WorktreeTarget {
    /// Create a branch from an optional base reference.
    BranchOff {
        /// Requested branch and directory name seed.
        #[serde(rename = "newBranch")]
        new_branch: String,
        /// Optional base reference.
        base: Option<String>,
    },
    /// Check out an existing local branch in a managed worktree.
    CheckoutBranch {
        /// Existing branch name.
        branch: String,
    },
    /// Forge pull request checkout, requiring an installed forge checkout adapter.
    CheckoutPr {
        /// Positive pull request number.
        #[serde(rename = "prNumber")]
        pr_number: u64,
    },
}

/// Legacy Git placement settings retained by the current upstream protocol.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitOptions {
    /// Base branch override.
    pub(crate) base_branch: Option<String>,
    /// Request a new branch.
    #[serde(default)]
    pub(crate) create_new_branch: bool,
    /// New branch name seed.
    pub(crate) new_branch_name: Option<String>,
    /// Create a managed checkout for this Agent.
    #[serde(default)]
    pub(crate) create_worktree: bool,
    /// Directory and branch name seed.
    pub(crate) worktree_slug: Option<String>,
    /// Base or checkout reference.
    pub(crate) ref_name: Option<String>,
    /// Branch creation or existing checkout.
    pub(crate) action: Option<GitAction>,
    /// Forge-specific change request selection.
    pub(crate) checkout_source:
        Option<domain::workspace::protocol::worktree_source::ChangeRequestCheckoutSource>,
    /// Legacy GitHub change request selection.
    pub(crate) github_pr_number: Option<u64>,
}

/// Legacy worktree action.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GitAction {
    /// Create a fresh branch.
    BranchOff,
    /// Use an existing branch.
    Checkout,
}
