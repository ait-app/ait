//! Project and Workspace registry inputs, mutations, failures and opaque identities.

use crate::workspace::records::{
    PersistedProjectKind, PersistedProjectRecord, PersistedWorkspaceRecord,
};

/// A registry operation failed before committing, or a project observer failed after commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// Input does not satisfy the source record schema.
    #[error("invalid registry record")]
    InvalidRecord,
    /// Existing state could not be decoded; it was not replaced by an empty registry.
    #[error("invalid registry file")]
    InvalidFile,
    /// Reading or atomically replacing the file failed.
    #[error("registry I/O failed")]
    Io,
    /// Mutation was explicitly frozen until a new registry instance is created.
    #[error("registry mutations are blocked until restart")]
    Frozen,
    /// A project observer failed after the mutation was committed.
    #[error("project mutation committed but observer failed")]
    Observer,
}

/// Generate an opaque `wks_` identity with eight cryptographically random bytes.
/// # Errors
/// Returns `Io` when the operating system cannot supply random bytes.
pub fn generate_workspace_id() -> Result<String, RegistryError> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|_| RegistryError::Io)?;
    Ok(format!("wks_{:016x}", u64::from_be_bytes(bytes)))
}

/// Source lifecycle mutation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// A record was inserted or updated.
    Upsert,
    /// An existing record was archived.
    Archive,
    /// An existing record was removed.
    Remove,
}

/// A project event published after the durable write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMutation {
    /// Mutation category.
    pub kind: MutationKind,
    /// Target identity, including when removed.
    pub project_id: String,
    /// Committed record, or none on removal.
    pub project: Option<PersistedProjectRecord>,
}

/// A workspace event published after the durable write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMutation {
    /// Mutation category.
    pub kind: MutationKind,
    /// Target identity, including when removed.
    pub workspace_id: String,
    /// Committed record, or none on removal.
    pub workspace: Option<PersistedWorkspaceRecord>,
    /// Present only when explicitly true on upsert.
    pub expects_initial_agent: Option<bool>,
}

/// Context accompanying a workspace insertion.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceMutationContext {
    /// A first agent is expected to follow this workspace creation.
    pub expects_initial_agent: Option<bool>,
}

/// Context accompanying a workspace archive.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceArchiveContext {
    /// Merged change request whose automatic archive was consumed.
    pub auto_archived_change_request_url: Option<String>,
}

/// Input to source-compatible active-root project allocation.
#[derive(Debug, Clone)]
pub struct ActiveProjectInput {
    /// Lexical root path; symlinks are not resolved by the registry.
    pub root_path: String,
    /// Current root classification.
    pub kind: PersistedProjectKind,
    /// Derived name for a new project only.
    pub display_name: String,
    /// Current project grouping key.
    pub project_key: Option<String>,
    /// Timestamp string assigned by the caller.
    pub timestamp: String,
}
