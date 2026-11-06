//! DSH's native interactive Host, isolated from the automation-only ACP adapter.
pub(super) mod commands;
pub(super) mod config;
mod content;
mod controls;
pub(super) mod discovery;
pub(super) mod exports;
pub(super) mod history;
mod http;
mod interactions;
pub(super) mod native_sessions;
pub(super) mod presets;
mod projection;
pub(super) mod quota;
mod runtime;
pub(super) mod session;
mod usage;

#[cfg(all(test, unix))]
mod tests;
