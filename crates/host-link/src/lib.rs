//! Center-managed Host publication. Adapters own HTTP, private storage and Relay transport.
pub mod authorization;
pub mod control;
mod controller;
mod ports;
mod types;

pub use controller::Controller;
pub mod service;
pub use ports::{Center, CredentialStore, ManagedRelay};
pub use types::{Binding, Credential, Error, Machine, Pending, Session, State, Tokens};

/// The only production authority for unattended machines.
pub const CENTER: &str = "https://aitapp.cn/api";
/// Override the center for local compatibility testing; production remains the default.
#[must_use]
pub fn center() -> String {
    std::env::var("AIT_CENTER_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map_or_else(
            || CENTER.to_owned(),
            |value| value.trim().trim_end_matches('/').to_owned(),
        )
}
/// The browser destination for human approval; never supplied by a token.
pub const VERIFICATION_URI: &str = "https://aitapp.cn/auth/device";
