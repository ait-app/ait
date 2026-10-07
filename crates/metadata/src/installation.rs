//! Complete metadata service installed as one capability component.

use std::sync::{Arc, Mutex};

use crate::service::{
    daemon::Daemon, directory::Directory, push::PushTokens,
    workspace_automation::WorkspaceAutomation, workspace_labels::WorkspaceLabels,
    workspace_names::WorkspaceNames, workspace_state::WorkspaceState,
};

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
