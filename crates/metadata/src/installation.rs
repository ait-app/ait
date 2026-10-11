//! Complete metadata service installed as one capability component.

use std::sync::{Arc, Mutex};

use crate::automation::service::workspace_automation::WorkspaceAutomation;
use crate::daemon::service::daemon::Daemon;
use crate::directory::service::{
    directory::Directory, github_projects::GithubProjects, workspace_names::WorkspaceNames,
    workspace_state::WorkspaceState,
};
use crate::labels::service::workspace_labels::WorkspaceLabels;
use crate::push::service::push::PushTokens;
use crate::worktrees::service::{workspace_recovery::WorkspaceRecovery, worktrees::Worktrees};

/// Required functional parts of the metadata service, sharing the composed Workspace registries.
#[derive(Debug)]
pub struct Dependencies {
    /// Durable push registration.
    pub push_tokens: PushTokens,
    /// Daemon configuration and lifecycle use cases.
    pub daemon: Daemon,
    /// Project and Workspace directory.
    pub directory: Directory,
    /// Workspace labels and subscriptions.
    pub workspace_labels: WorkspaceLabels,
    /// Shared setup and script lifecycle.
    pub workspace_automation: Arc<Mutex<WorkspaceAutomation>>,
    /// Workspace attention use cases.
    pub workspace_state: WorkspaceState,
    /// Shared background title and branch naming.
    pub workspace_names: WorkspaceNames,
    /// Shared managed worktree lifecycle, also used by directory creation and schedules.
    pub worktrees: Arc<Mutex<Worktrees>>,
    /// Archived Workspace recovery.
    pub workspace_recovery: WorkspaceRecovery,
    /// GitHub repository discovery and Project cloning.
    pub github_projects: GithubProjects,
}

/// One complete metadata service; connection protocol helpers remain available to the API.
#[derive(Debug)]
pub struct Service {
    dependencies: Dependencies,
}

impl Service {
    /// Construct the service from all required `dependencies`, retaining existing shared handles.
    /// # Returns
    /// A complete installation with independently synchronized functional parts.
    #[must_use]
    pub fn new(dependencies: Dependencies) -> Self {
        Self { dependencies }
    }

    /// Transfer the complete service into the host's dispatch and lifecycle composition.
    /// # Returns
    /// All functional parts without replacing shared registries, locks, or background tasks.
    #[must_use]
    pub fn into_dependencies(self) -> Dependencies {
        self.dependencies
    }
}
