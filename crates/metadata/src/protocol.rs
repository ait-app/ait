//! Metadata protocol owned by the independent server.

pub(crate) mod daemon;
pub(crate) mod directory;
pub(crate) mod github_projects;
pub(crate) mod project_config;
pub(crate) mod project_icon;
pub mod server;
pub(crate) mod workspace_automation;
pub(crate) mod workspace_labels;
pub(crate) mod workspace_recovery;
pub(crate) mod workspace_state;
pub(crate) mod worktrees;

/// Leased push token management.
pub(crate) mod push;
