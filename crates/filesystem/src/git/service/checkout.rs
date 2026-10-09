//! Checkout status, diff, refresh, and history use cases.

use std::path::Path;
use std::sync::Arc;

use model::workspace::registry::WorkspaceRegistry;

use crate::git::ports::checkout::{
    CheckoutBranchResolution, CheckoutBranchSource, CheckoutBranchSuggestion, CheckoutCommits,
    CheckoutDiff, CheckoutDiffCompare, CheckoutFailureKind, CheckoutMergeStrategy, CheckoutRuntime,
    CheckoutRuntimeError, CheckoutStashEntry, CheckoutStatus, ParsedDiffFile,
};

/// Thin application boundary over the independent blocking Git adapter.
#[derive(Debug)]
pub struct Checkout {
    runtime: Box<dyn CheckoutRuntime>,
    workspace_registry: Option<Arc<dyn WorkspaceRegistry>>,
}

impl Checkout {
    /// Compose checkout use cases.
    #[must_use]
    pub fn new(runtime: Box<dyn CheckoutRuntime>) -> Self {
        Self {
            runtime,
            workspace_registry: None,
        }
    }

    /// Attach the durable workspace registry used to validate and record resets.
    #[must_use]
    pub fn with_workspace_registry(mut self, registry: Arc<dyn WorkspaceRegistry>) -> Self {
        self.workspace_registry = Some(registry);
        self
    }

    /// Inspect checkout status.
    ///
    /// # Errors
    /// Returns categorized local Git/filesystem failures.
    pub(crate) fn status(&self, cwd: &str) -> Result<CheckoutStatus, CheckoutRuntimeError> {
        self.runtime.status(cwd)
    }

    /// Force a fresh checkout read.
    ///
    /// # Errors
    /// Returns categorized local Git/filesystem failures.
    pub(crate) fn refresh(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        self.runtime.refresh(cwd)
    }

    /// Read a structured checkout diff.
    ///
    /// # Errors
    /// Returns categorized local Git/filesystem failures.
    pub(crate) fn diff(
        &self,
        cwd: &str,
        compare: &CheckoutDiffCompare,
    ) -> Result<CheckoutDiff, CheckoutRuntimeError> {
        self.runtime.diff(cwd, compare)
    }

    /// List checkout commits plus bounded base context.
    ///
    /// # Errors
    /// Returns categorized local Git/filesystem failures.
    pub(crate) fn commits(&self, cwd: &str) -> Result<CheckoutCommits, CheckoutRuntimeError> {
        self.runtime.commits(cwd)
    }

    /// Read one textual file diff for a commit.
    ///
    /// # Errors
    /// Returns categorized input, Git, or filesystem failures.
    pub(crate) fn commit_file_diff(
        &self,
        cwd: &str,
        sha: &str,
        path: &str,
    ) -> Result<Option<ParsedDiffFile>, CheckoutRuntimeError> {
        self.runtime.commit_file_diff(cwd, sha, path)
    }

    /// Resolve a local or origin branch.
    ///
    /// # Errors
    /// Returns categorized validation or Git failures.
    pub(crate) fn validate_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<CheckoutBranchResolution, CheckoutRuntimeError> {
        self.runtime.validate_branch(cwd, branch)
    }

    /// List branch suggestions.
    ///
    /// # Errors
    /// Returns categorized validation or Git failures.
    pub(crate) fn branch_suggestions(
        &self,
        cwd: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<CheckoutBranchSuggestion>, CheckoutRuntimeError> {
        self.runtime.branch_suggestions(cwd, query, limit)
    }

    /// Check out an existing branch.
    ///
    /// # Errors
    /// Returns categorized dirty-tree, validation, or Git failures.
    pub(crate) fn switch_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<CheckoutBranchSource, CheckoutRuntimeError> {
        self.runtime.switch_branch(cwd, branch)
    }

    /// Rename the current branch.
    ///
    /// # Errors
    /// Returns categorized detached-head, validation, or Git failures.
    pub(crate) fn rename_branch(
        &self,
        cwd: &str,
        branch: &str,
    ) -> Result<String, CheckoutRuntimeError> {
        self.runtime.rename_branch(cwd, branch)
    }

    /// Commit checkout changes.
    ///
    /// # Errors
    /// Returns categorized validation or Git failures.
    pub(crate) fn commit(
        &self,
        cwd: &str,
        message: &str,
        add_all: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        self.runtime.commit(cwd, message, add_all)
    }

    /// Merge the current branch into its base checkout.
    ///
    /// # Errors
    /// Returns categorized preflight, conflict, or Git failures.
    pub(crate) fn merge_to_base(
        &self,
        cwd: &str,
        base_ref: Option<&str>,
        strategy: CheckoutMergeStrategy,
        require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        self.runtime
            .merge_to_base(cwd, base_ref, strategy, require_clean_target)
    }

    /// Merge the selected base into the current branch.
    ///
    /// # Errors
    /// Returns categorized preflight, conflict, or Git failures.
    pub(crate) fn merge_from_base(
        &self,
        cwd: &str,
        base_ref: Option<&str>,
        require_clean_target: bool,
    ) -> Result<(), CheckoutRuntimeError> {
        self.runtime
            .merge_from_base(cwd, base_ref, require_clean_target)
    }

    /// Reset a managed workspace to origin's latest default branch on its initial branch.
    ///
    /// # Errors
    /// Returns categorized local Git and remote failures.
    pub(crate) fn reset_workspace(
        &self,
        cwd: &str,
        workspace_id: &str,
        initial_branch: &str,
    ) -> Result<(), CheckoutRuntimeError> {
        if let Some(registry) = &self.workspace_registry {
            let workspace = registry
                .get(workspace_id)
                .map_err(registry_reset_error)?
                .ok_or_else(|| invalid_reset_workspace("Workspace not found"))?;
            // Clients normalize trailing separators in registered workspace paths.
            if Path::new(&workspace.cwd) != Path::new(cwd)
                || workspace.archived_at.is_some()
                || !workspace.is_paseo_owned_worktree
                || workspace.display_name != initial_branch
            {
                return Err(invalid_reset_workspace(
                    "Workspace identity or initial branch mismatch",
                ));
            }
        }
        self.runtime.reset_workspace(cwd, initial_branch)?;
        if let Some(registry) = &self.workspace_registry {
            let updated = registry
                .update(workspace_id, &|workspace| {
                    let mut updated = workspace.clone();
                    if Path::new(&updated.cwd) == Path::new(cwd)
                        && updated.archived_at.is_none()
                        && updated.is_paseo_owned_worktree
                        && updated.display_name == initial_branch
                    {
                        updated.branch = Some(initial_branch.to_owned());
                        updated.updated_at =
                            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                    }
                    updated
                })
                .map_err(registry_reset_error)?
                .ok_or_else(|| invalid_reset_workspace("Workspace record disappeared"))?;
            if updated.branch.as_deref() != Some(initial_branch) {
                return Err(invalid_reset_workspace(
                    "Workspace record changed during reset",
                ));
            }
        }
        Ok(())
    }

    /// Pull the current branch.
    ///
    /// # Errors
    /// Returns categorized remote, conflict, or Git failures.
    pub(crate) fn pull(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        self.runtime.pull(cwd)
    }

    /// Push the current branch.
    ///
    /// # Errors
    /// Returns categorized remote or Git failures.
    pub(crate) fn push(&self, cwd: &str) -> Result<(), CheckoutRuntimeError> {
        self.runtime.push(cwd)
    }

    /// Discard selected checkout paths.
    ///
    /// # Errors
    /// Returns categorized path or Git failures.
    pub(crate) fn discard_changes(
        &self,
        cwd: &str,
        paths: &[String],
    ) -> Result<(), CheckoutRuntimeError> {
        self.runtime.discard_changes(cwd, paths)
    }

    /// Save an Ait-tagged stash.
    ///
    /// # Errors
    /// Returns categorized Git failures.
    pub(crate) fn stash_save(
        &self,
        cwd: &str,
        branch: Option<&str>,
    ) -> Result<(), CheckoutRuntimeError> {
        self.runtime.stash_save(cwd, branch)
    }

    /// Pop a stash.
    ///
    /// # Errors
    /// Returns categorized conflict or Git failures.
    pub(crate) fn stash_pop(&self, cwd: &str, index: usize) -> Result<(), CheckoutRuntimeError> {
        self.runtime.stash_pop(cwd, index)
    }

    /// List stashes.
    ///
    /// # Errors
    /// Returns categorized Git failures.
    pub(crate) fn stashes(
        &self,
        cwd: &str,
        paseo_only: bool,
    ) -> Result<Vec<CheckoutStashEntry>, CheckoutRuntimeError> {
        self.runtime.stashes(cwd, paseo_only)
    }
}

fn invalid_reset_workspace(message: &str) -> CheckoutRuntimeError {
    CheckoutRuntimeError {
        kind: CheckoutFailureKind::NotAllowed,
        message: message.to_owned(),
    }
}

fn registry_reset_error(
    _error: domain::workspace::registry::RegistryError,
) -> CheckoutRuntimeError {
    CheckoutRuntimeError {
        kind: CheckoutFailureKind::Unknown,
        message: "Unable to update workspace record".to_owned(),
    }
}

#[cfg(test)]
mod tests;
