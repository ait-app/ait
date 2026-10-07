//! Shared definitions are owned by the model crate.

pub use model::workspace::protocol::workspace::{
    AheadBehind, CheckStatus, ChecksStatus, DiffStat, Mergeable, ProjectCheckoutLitePayload,
    ProjectKind, ProjectPlacementPayload, ReviewDecision, WorkspaceCheck,
    WorkspaceDescriptorPayload, WorkspaceGitHubRuntimePayload, WorkspaceGitRuntimePayload,
    WorkspaceKind, WorkspaceProjectDescriptorPayload, WorkspacePullRequest, WorkspaceRuntimeError,
    WorkspaceScriptHealth, WorkspaceScriptLifecycle, WorkspaceScriptPayload, WorkspaceScriptType,
    WorkspaceStateBucket,
};
