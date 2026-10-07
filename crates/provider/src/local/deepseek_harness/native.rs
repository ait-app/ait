//! DSH's native interactive Host, isolated from the automation-only ACP adapter.
mod config;
mod content;
pub(super) mod history;
mod http;
mod interactions;
pub(super) mod native_sessions;
mod projection;
mod runtime;
mod session;
mod usage;

pub(super) use config::validate;
pub(super) use session::open;

#[cfg(all(test, unix))]
mod tests;

/// Advertise built-in presets; the Host catalog validates every requested selection.
pub(super) fn modes() -> serde_json::Value {
    serde_json::json!([
        {"id":"read-only","label":"Read only"},
        {"id":"workspace-write","label":"Workspace write"},
        {"id":"danger-full-access","label":"Full access"}
    ])
}
