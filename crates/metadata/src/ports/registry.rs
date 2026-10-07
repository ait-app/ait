//! Shared definitions are owned by the model crate.

pub use model::workspace::registry::{
    ActiveProjectInput, MutationKind, MutationListener, MutationSubscription, ProjectMutation,
    ProjectRegistry, RegistryError, WorkspaceArchiveContext, WorkspaceMutation,
    WorkspaceMutationContext, WorkspaceRegistry,
};
