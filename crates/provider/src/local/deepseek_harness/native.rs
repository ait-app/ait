//! DSH's native interactive Host, isolated from the automation-only ACP adapter.
pub(super) mod commands;
mod config;
mod content;
mod controls;
mod discovery;
pub(super) mod exports;
pub(super) mod history;
mod http;
mod interactions;
pub(super) mod native_sessions;
mod presets;
mod projection;
pub(super) mod quota;
mod runtime;
mod session;
mod usage;

pub(super) use config::validate;
pub(super) use discovery::discover;
pub(super) use presets::validate_selection;
pub(super) use session::{open, preview};

#[cfg(all(test, unix))]
mod tests;
