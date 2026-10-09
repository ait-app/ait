//! Blocking registry contracts corresponding to Paseo workspace-registry.ts.

use std::fmt::Debug;
use std::sync::Arc;

use domain::workspace::records::{PersistedProjectRecord, PersistedWorkspaceRecord};
use domain::workspace::registry::{
    ActiveProjectInput, ProjectMutation, RegistryError, WorkspaceArchiveContext, WorkspaceMutation,
    WorkspaceMutationContext,
};

/// Post-commit observer. A workspace ignores observer errors; a project reports them.
pub type MutationListener<T> = Arc<dyn Fn(&T) -> Result<(), RegistryError> + Send + Sync>;

/// Dropping this registration unsubscribes the observer.
pub trait MutationSubscription: Debug + Send {}

/// Project registry. Calls are blocking and must run outside an async reactor.
/// Mutations on one instance are serialized; the host owns cross-process exclusion.
pub trait ProjectRegistry: Debug + Send + Sync {
    /// Load lazily. Errors preserve existing state and leave initialization retryable.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn initialize(&self) -> Result<(), RegistryError>;
    /// Check the registry file's existence without creating it.
    fn exists_on_disk(&self) -> bool;
    /// Return insertion-ordered records, including archived records.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn list(&self) -> Result<Vec<PersistedProjectRecord>, RegistryError>;
    /// Return a record or none for a missing identity.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn get(&self, id: &str) -> Result<Option<PersistedProjectRecord>, RegistryError>;
    /// Reuse the oldest active equivalent root, or allocate a new `prj_` identity.
    ///
    /// # Errors
    /// Returns storage/validation errors; observer errors occur after commit.
    fn get_or_create_active_by_root(
        &self,
        input: &ActiveProjectInput,
    ) -> Result<PersistedProjectRecord, RegistryError>;
    /// Validate and atomically insert/replace a record, then notify observers.
    ///
    /// # Errors
    /// Returns storage/validation errors; observer errors occur after commit.
    fn upsert(&self, record: &PersistedProjectRecord) -> Result<(), RegistryError>;
    /// Transform the latest record under serialization; missing identities return none.
    ///
    /// # Errors
    /// Returns storage/validation errors; observer errors occur after commit.
    fn update(
        &self,
        id: &str,
        update: &dyn Fn(&PersistedProjectRecord) -> PersistedProjectRecord,
    ) -> Result<Option<PersistedProjectRecord>, RegistryError>;
    /// Archive once; repeated archives and missing identities do not notify observers.
    ///
    /// # Errors
    /// Returns storage/validation errors; observer errors occur after commit.
    fn archive(&self, id: &str, timestamp: &str) -> Result<(), RegistryError>;
    /// Remove if present, notifying only after a committed change.
    ///
    /// # Errors
    /// Returns storage/validation errors; observer errors occur after commit.
    fn remove(&self, id: &str) -> Result<(), RegistryError>;
    /// Subscribe until the returned handle is dropped.
    fn subscribe_to_mutations(
        &self,
        listener: MutationListener<ProjectMutation>,
    ) -> Box<dyn MutationSubscription>;
}

/// Workspace registry with independent identities, including multiple records at one cwd.
pub trait WorkspaceRegistry: Debug + Send + Sync {
    /// Load lazily; a malformed file is an error and remains untouched.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn initialize(&self) -> Result<(), RegistryError>;
    /// Check the registry file's existence without creating it.
    fn exists_on_disk(&self) -> bool;
    /// Return insertion-ordered records, including archived records.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn list(&self) -> Result<Vec<PersistedWorkspaceRecord>, RegistryError>;
    /// Return a record or none for a missing identity.
    ///
    /// # Errors
    /// Returns load errors or an unavailable/poisoned registry.
    fn get(&self, id: &str) -> Result<Option<PersistedWorkspaceRecord>, RegistryError>;
    /// Validate and atomically insert/replace a record, then notify observers.
    ///
    /// # Errors
    /// Returns validation, load, write or frozen-registry errors.
    fn upsert(
        &self,
        record: &PersistedWorkspaceRecord,
        context: WorkspaceMutationContext,
    ) -> Result<(), RegistryError>;
    /// Transform the latest record under serialization; missing identities return none.
    ///
    /// # Errors
    /// Returns validation, load, write or frozen-registry errors.
    fn update(
        &self,
        id: &str,
        update: &dyn Fn(&PersistedWorkspaceRecord) -> PersistedWorkspaceRecord,
    ) -> Result<Option<PersistedWorkspaceRecord>, RegistryError>;
    /// Refresh archive timestamps, preserving existing auto-archive metadata unless replaced.
    ///
    /// # Errors
    /// Returns validation, load, write or frozen-registry errors.
    fn archive(
        &self,
        id: &str,
        timestamp: &str,
        context: &WorkspaceArchiveContext,
    ) -> Result<(), RegistryError>;
    /// Remove if present, notifying only after a committed change.
    ///
    /// # Errors
    /// Returns validation, load, write or frozen-registry errors.
    fn remove(&self, id: &str) -> Result<(), RegistryError>;
    /// Subscribe until the returned handle is dropped; observer errors cannot undo a commit.
    fn subscribe_to_mutations(
        &self,
        listener: MutationListener<WorkspaceMutation>,
    ) -> Box<dyn MutationSubscription>;
    /// Reject further mutations until this instance is replaced; reads remain available.
    ///
    /// # Errors
    /// Returns a load error or an unavailable/poisoned registry.
    fn block_all_mutations_until_restart(&self) -> Result<(), RegistryError>;
}
