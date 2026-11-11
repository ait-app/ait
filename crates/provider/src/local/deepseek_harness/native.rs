//! DSH's native interactive Host, isolated from the automation-only ACP adapter.
pub(super) mod config;
mod content;
pub(super) mod discovery;
pub(super) mod history;
mod http;
mod interactions;
mod mcp;
pub(super) mod native_sessions;
pub(super) mod presets;
pub(super) mod preview;
mod projection;
pub(super) mod quota;
mod runtime;
pub(super) mod session;
mod usage;

#[cfg(all(test, unix))]
mod tests;
