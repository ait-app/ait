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

/// GitHub clone transport selected by the caller or host configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubCloneProtocol {
    /// Clone over HTTPS.
    Https,
    /// Clone over SSH.
    Ssh,
}

/// Observed stage of a running GitHub clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubClonePhase {
    /// Preparing the staged checkout.
    Starting,
    /// Counting objects in the remote repository.
    Counting,
    /// Compressing objects in the remote repository.
    Compressing,
    /// Receiving objects from the remote repository.
    Receiving,
    /// Resolving received objects into deltas.
    Resolving,
    /// Writing the working tree.
    Checkout,
    /// Registering the completed checkout as a Project.
    Registering,
}

impl GithubClonePhase {
    /// Fraction of the overall clone this phase spans, as `(start, end)` percentages.
    const fn bounds(self) -> (u8, u8) {
        match self {
            Self::Starting => (0, 0),
            Self::Counting => (1, 4),
            Self::Compressing => (4, 8),
            Self::Receiving => (8, 80),
            Self::Resolving => (80, 92),
            Self::Checkout => (92, 98),
            Self::Registering => (99, 99),
        }
    }
}

/// One bounded progress observation for a running GitHub clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GithubCloneProgress {
    /// Stage reported by Git or by Project registration.
    pub phase: GithubClonePhase,
    /// Completion within `phase`, when the source reported one.
    pub phase_percent: Option<u8>,
}

impl GithubCloneProgress {
    /// Build an observation, clamping any phase percent to `0..=100`.
    #[must_use]
    pub fn new(phase: GithubClonePhase, phase_percent: Option<u8>) -> Self {
        Self {
            phase,
            phase_percent: phase_percent.map(|percent| percent.min(100)),
        }
    }

    /// Overall completion in `0..=100`, weighted across the clone phases.
    #[must_use]
    pub fn overall_percent(self) -> u8 {
        let (start, end) = self.phase.bounds();
        let Some(percent) = self.phase_percent else {
            return start;
        };
        let span = u16::from(end - start);
        let offset = u8::try_from(span * u16::from(percent) / 100).unwrap_or(u8::MAX);
        start + offset
    }
}

/// Normalized repository row returned by GitHub CLI discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GithubRepository {
    /// GitHub GraphQL identity or CLI numeric identity as text.
    pub id: String,
    /// Repository name without owner.
    pub name: String,
    /// Full owner/repository path.
    pub name_with_owner: String,
    /// Optional description.
    pub description: Option<String>,
    /// Public or private visibility.
    pub visibility: GithubRepositoryVisibility,
    /// GitHub update timestamp.
    pub updated_at: String,
    /// Clone URL honoring the host Git protocol setting.
    pub clone_url: String,
}

/// Visibility reported by the available GitHub CLI repository commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubRepositoryVisibility {
    /// Public repository.
    Public,
    /// Private repository.
    Private,
}

/// Safe failure category for GitHub project operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GithubProjectsError {
    /// `gh` is absent from the server PATH.
    #[error("GitHub CLI (gh) is not installed or not in PATH")]
    CliMissing,
    /// `gh` needs a host login.
    #[error("GitHub CLI is not authenticated. Run gh auth login on the host.")]
    Unauthenticated,
    /// Repository discovery returned invalid data or otherwise failed.
    #[error("GitHub repository search failed")]
    SearchFailed,
    /// The requested clone destination is unsafe or unavailable.
    #[error("Invalid clone target directory")]
    InvalidTarget,
    /// The destination already exists.
    #[error("Checkout path already exists")]
    TargetExists,
    /// Git clone failed without publishing a checkout.
    #[error("GitHub repository clone failed")]
    CloneFailed,
}

#[cfg(test)]
mod tests;
