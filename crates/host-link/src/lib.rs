//! Center-managed Host publication. Adapters own HTTP, private storage and Relay transport.
pub mod authorization;
pub mod control;
mod controller;
mod ports;
mod types;

pub mod service;
pub use ports::{Center, CredentialStore, ManagedRelay};
pub use types::{
    Binding, BulkTickets, Credential, Error, HubTicket, Machine, Pending, Session, State, Tokens,
};

/// The default official entry for unattended machines.
pub const CENTER: &str = "https://aitapp.cn/api";
/// Override the center for local compatibility testing; production remains the default.
#[must_use]
pub fn center() -> String {
    center_override().unwrap_or_else(|| CENTER.to_owned())
}

/// Read the explicit center override, primarily for local compatibility testing.
///
/// Returns the trimmed URL without trailing slashes, or `None` if unset or blank.
#[must_use]
pub fn center_override() -> Option<String> {
    std::env::var("AIT_CENTER_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().trim_end_matches('/').to_owned())
}

/// The browser destination for human approval; never supplied by a token.
pub const VERIFICATION_URI: &str = "https://aitapp.cn/auth/device";
