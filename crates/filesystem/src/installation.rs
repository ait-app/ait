//! Complete filesystem service installed as one capability component.

use std::sync::{Arc, Mutex};

use crate::service::{
    checkout::Checkout, files::Files, forge::Forge, git_fetch::GitFetch,
    github_projects::GithubProjects, skills::Skills, workspace_recovery::WorkspaceRecovery,
    worktrees::Worktrees,
};

/// Required functional parts of the filesystem service; locks remain independently scoped.
#[derive(Debug)]
pub struct Dependencies {
    /// Git checkout operations.
    pub checkout: Checkout,
    /// Forge operations.
    pub forge: Forge,
    /// Scoped file operations and download grants.
    pub files: Files,
    /// Repository discovery and cloning.
    pub github_projects: GithubProjects,
    /// Shared managed worktree lifecycle.
    pub worktrees: Arc<Mutex<Worktrees>>,
    /// Archived workspace recovery.
    pub workspace_recovery: WorkspaceRecovery,
    /// Skill selection and installation.
    pub skills: Skills,
    /// Background fetches for observed repositories.
    pub git_fetch: GitFetch,
}

/// One complete filesystem service, installed and advertised at crate granularity.
#[derive(Debug)]
pub struct Service {
    dependencies: Dependencies,
}

impl Service {
    /// Construct the service from all required `dependencies`, without starting background work.
    /// # Returns
    /// A complete installation retaining each part's independent state and synchronization.
    #[must_use]
    pub fn new(dependencies: Dependencies) -> Self {
        Self { dependencies }
    }

    /// Borrow functional parts for cross-component composition.
    /// # Returns
    /// The required dependencies; borrowing does not install or remove individual capabilities.
    #[must_use]
    pub fn dependencies(&self) -> &Dependencies {
        &self.dependencies
    }

    /// Transfer the complete service into the host's dispatch and lifecycle composition.
    /// # Returns
    /// All functional parts, preserving their original resources and ownership.
    #[must_use]
    pub fn into_dependencies(self) -> Dependencies {
        self.dependencies
    }
}
