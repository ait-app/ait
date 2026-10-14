//! Filesystem and Git observations used by project/workspace provisioning.

use std::fmt::Debug;

use domain::workspace::provisioning::{
    Checkout, DirectorySourceError, GithubProjectsError, GithubRepository,
};

/// Blocking adapter for local directory and lightweight Git inspection.
pub trait DirectorySource: Debug + Send + Sync {
    /// Resolve an existing directory and inspect its checkout placement.
    ///
    /// # Errors
    /// Returns a categorized filesystem error without modifying the directory.
    fn inspect(&self, path: &str) -> Result<Checkout, DirectorySourceError>;

    /// Create one empty child directory below an already normalized parent.
    ///
    /// # Errors
    /// Returns a categorized filesystem error without recursive creation.
    fn create_child(&self, parent: &str, name: &str) -> Result<String, DirectorySourceError>;

    /// Remove a directory only when it is empty.
    ///
    /// # Errors
    /// Returns a categorized filesystem error; non-empty directories are preserved.
    fn remove_empty(&self, path: &str) -> Result<(), DirectorySourceError>;

    /// Compare directory identities with realpath awareness where both paths exist.
    fn equivalent(&self, left: &str, right: &str) -> bool;

    /// Return the realpath spelling of an existing directory.
    ///
    /// # Errors
    /// Returns a categorized filesystem error for missing or unreadable paths.
    fn canonical(&self, path: &str) -> Result<String, DirectorySourceError>;
}

/// Blocking GitHub CLI and Git clone operations for Project provisioning; callers run this outside
/// the async reactor.
pub trait GithubProjectsRuntime: Debug + Send + Sync {
    /// List recent owned repositories for an empty query, otherwise search accessible repos.
    ///
    /// # Errors
    /// Returns a categorized CLI, authentication, or parsing failure.
    fn search_repositories(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<GithubRepository>, GithubProjectsError>;

    /// Resolve the checkout path before cloning, so failures can report the intended path.
    ///
    /// # Errors
    /// Returns an invalid-target error when the parent path or child name cannot be resolved.
    fn checkout_path(
        &self,
        target_directory: &str,
        name: &str,
    ) -> Result<String, GithubProjectsError>;

    /// Clone one validated GitHub URL into a new child under `target_directory`.
    ///
    /// The adapter creates missing parent directories, stages the clone, and atomically renames
    /// the completed checkout. It checks for an existing target before the rename; simultaneous
    /// cross-process creation still needs a future no-replace primitive.
    ///
    /// # Errors
    /// Returns a categorized path, collision, or clone failure.
    fn clone_repository(
        &self,
        clone_url: &str,
        target_directory: &str,
        name: &str,
    ) -> Result<String, GithubProjectsError>;
}
