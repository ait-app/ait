//! Agent protocol owned by the independent server.

pub(crate) mod agent;
pub mod agent_config;
pub(crate) mod agent_execution;
pub(crate) mod agent_lifecycle;

pub(crate) mod controls;
pub(crate) mod creation;
/// Existing native session operations.
pub(crate) mod native_sessions;
pub mod prompt;
pub mod provider;
pub(crate) mod resume;
pub mod timeline;
pub mod usage;
