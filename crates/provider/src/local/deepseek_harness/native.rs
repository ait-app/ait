//! DSH's native interactive Host, isolated from the automation-only ACP adapter.
mod config;
mod content;
mod discovery;
pub(super) mod history;
mod http;
mod interactions;
pub(super) mod native_sessions;
mod presets;
mod projection;
mod runtime;
mod session;
mod usage;

pub(super) use config::validate;
pub(super) use discovery::discover;
pub(super) use presets::validate_selection;
pub(super) use session::open;

#[cfg(all(test, unix))]
mod tests;
