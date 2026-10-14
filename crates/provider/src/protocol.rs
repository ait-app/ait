//! Agent protocol owned by the independent server.

pub(crate) mod agent;
pub(crate) mod agent_config;
pub(crate) mod agent_execution;
pub(crate) mod agent_lifecycle;

pub(crate) mod controls;
pub(crate) mod creation;
/// Existing native session operations.
pub(crate) mod native_sessions;
pub(crate) mod prompt;
pub(crate) mod provider;
pub(crate) mod resume;
pub(crate) mod timeline;
pub(crate) mod usage;
