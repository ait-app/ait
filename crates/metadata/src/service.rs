//! Metadata service owned by the independent server.

pub mod daemon;
pub mod directory;
pub mod workspace_automation;
pub mod workspace_labels;
pub mod workspace_state;

/// Leased push token management.
pub mod push;

/// Background workspace title and placeholder branch generation.
pub mod workspace_names;

/// Adapters for shared Workspace collaboration contracts.
pub mod workspace_collaboration;
