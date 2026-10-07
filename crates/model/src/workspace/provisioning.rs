//! Filesystem and Git observations used by project/workspace provisioning.

use std::fmt::Debug;

/// Preferred project setup and script configuration file.
pub const PROJECT_CONFIG_FILE_NAME: &str = "ait.json";
/// Read-only fallback for projects created before the Ait filename migration.
pub const LEGACY_PROJECT_CONFIG_FILE_NAME: &str = "paseo.json";

/// A normalized directory and its lightweight Git placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// Absolute selected directory. It can be below the Git worktree root.
    pub cwd: String,
    /// Whether the directory belongs to a Git checkout.
    pub is_git: bool,
    /// Current branch, or none for detached HEAD and non-Git directories.
    pub current_branch: Option<String>,
    /// Preferred remote URL, when one is configured.
    pub remote_url: Option<String>,
    /// Git worktree root. Non-Git directories have no root.
    pub worktree_root: Option<String>,
    /// Whether Paseo created and owns this linked worktree.
    pub is_paseo_owned_worktree: bool,
    /// Main checkout root for a linked worktree.
    pub main_repo_root: Option<String>,
}

/// Safe filesystem failure categories used by provisioning use cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DirectorySourceError {
    /// The requested path is absent or is not a directory.
    #[error("directory not found")]
    NotFound,
    /// The requested directory already exists.
    #[error("directory already exists")]
    AlreadyExists,
    /// The operation was denied by filesystem permissions.
    #[error("permission denied")]
    PermissionDenied,
    /// Another filesystem or Git inspection operation failed.
    #[error("filesystem inspection failed")]
    Io,
}

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
