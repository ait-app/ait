//! Normalized checkout placement, project configuration names and inspection failures.

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
