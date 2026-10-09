//! Workspace collaboration contracts without concrete capability services.

use std::fmt::Debug;
use std::sync::Arc;

use domain::summary::SummarySelection;
use domain::workspace::lifecycle::{WorkspaceCreation, WorkspaceLifecycleError};
use domain::workspace::records::{PersistedProjectRecord, PersistedWorkspaceRecord};

use crate::workspace::worktrees::WorktreeProvisioning;

/// Blocking Project registration after filesystem checkout provisioning.
pub trait ProjectRegistration: Debug + Send + Sync {
    /// Inspect and register the completed checkout at `path`, using `timestamp` for mutations.
    /// Returns the active Project, retaining existing identity and user metadata on reuse.
    /// # Errors
    /// Returns safe inspection, validation, or registry failures.
    fn register_project(
        &self,
        path: &str,
        timestamp: &str,
    ) -> Result<PersistedProjectRecord, WorkspaceLifecycleError>;
}

/// Blocking Workspace registration used by native Agent placement.
pub trait WorkspaceDirectory: Debug + Send + Sync {
    /// Register `creation` as a fresh Workspace and return its persisted placement.
    /// # Errors
    /// Returns inspection, validation, identity conflict, or persistence failures.
    fn create_workspace(
        &self,
        creation: WorkspaceCreation<'_>,
    ) -> Result<PersistedWorkspaceRecord, WorkspaceLifecycleError>;

    /// Reuse, restore, or register a Workspace at `path`, using `timestamp` for mutations.
    /// # Errors
    /// Returns inspection or persistence failures.
    fn open_workspace(
        &self,
        path: &str,
        timestamp: &str,
    ) -> Result<PersistedWorkspaceRecord, WorkspaceLifecycleError>;

    /// Return the shared observer wakeup resource, when directory pushes are installed.
    fn changes(&self) -> Option<crate::changes::Changes>;

    /// Return the existing shared Git provisioning adapter, when installed.
    fn shared_worktrees(&self) -> Option<Arc<dyn WorktreeProvisioning>>;
}

/// Setup trigger retaining the owning capability's tasks and synchronization.
pub trait WorkspaceSetup: Debug + Send + Sync {
    /// Start setup for `workspace_id`, returning whether a setup was started.
    /// # Errors
    /// Returns missing Workspace, configuration, registry, or runtime failures.
    fn start_created_setup(&self, workspace_id: &str) -> Result<bool, WorkspaceLifecycleError>;
}

/// Background naming trigger; eligibility and writeback belong to the implementation.
pub trait WorkspaceNaming: Debug + Send + Sync {
    /// Queue `context` for `workspace_id`, using optional provider/model `selection`.
    /// Returns immediately; the owner enforces admission, cancellation, and naming eligibility.
    fn schedule(&self, workspace_id: String, context: String, selection: Option<SummarySelection>);
}
