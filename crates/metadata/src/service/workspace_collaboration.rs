//! Workspace adapters retaining the capability's existing shared state and owners.

use std::sync::{Arc, Mutex};

use model::summary::SummarySelection;
use model::workspace::lifecycle::{
    WorkspaceCreation, WorkspaceDirectory, WorkspaceLifecycleError, WorkspaceNaming, WorkspaceSetup,
};
use model::workspace::records::PersistedWorkspaceRecord;
use model::workspace::worktrees::WorktreeProvisioning;

use super::directory::Directory;
use super::workspace_automation::WorkspaceAutomation;
use super::workspace_names::WorkspaceNames;

impl WorkspaceDirectory for Directory {
    fn create_workspace(
        &self,
        creation: WorkspaceCreation<'_>,
    ) -> Result<PersistedWorkspaceRecord, WorkspaceLifecycleError> {
        Directory::create_workspace(self, creation).map_err(failure)
    }

    fn open_workspace(
        &self,
        path: &str,
        timestamp: &str,
    ) -> Result<PersistedWorkspaceRecord, WorkspaceLifecycleError> {
        Directory::open_workspace(self, path, timestamp).map_err(failure)
    }

    fn changes(&self) -> Option<model::changes::Changes> {
        Directory::changes(self)
    }

    fn shared_worktrees(&self) -> Option<Arc<dyn WorktreeProvisioning>> {
        Directory::shared_worktrees(self)
    }
}

impl WorkspaceNaming for WorkspaceNames {
    fn schedule(&self, workspace_id: String, context: String, selection: Option<SummarySelection>) {
        WorkspaceNames::schedule(self, workspace_id, context, selection);
    }
}

/// Setup adapter sharing the lock and task owner used by Workspace RPCs.
#[derive(Debug, Clone)]
pub struct SharedWorkspaceSetup {
    automation: Arc<Mutex<WorkspaceAutomation>>,
}

impl SharedWorkspaceSetup {
    /// Wrap the existing `automation` service without creating another lock or runtime.
    /// # Returns
    /// An adapter whose setup requests use that same service instance.
    #[must_use]
    pub fn new(automation: Arc<Mutex<WorkspaceAutomation>>) -> Self {
        Self { automation }
    }
}

impl WorkspaceSetup for SharedWorkspaceSetup {
    fn start_created_setup(&self, workspace_id: &str) -> Result<bool, WorkspaceLifecycleError> {
        self.automation
            .lock()
            .map_err(failure)?
            .start_created_setup(workspace_id)
            .map_err(failure)
    }
}

fn failure(error: impl std::fmt::Display) -> WorkspaceLifecycleError {
    WorkspaceLifecycleError {
        message: error.to_string(),
    }
}
