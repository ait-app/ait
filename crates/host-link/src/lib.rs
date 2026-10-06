//! Center-managed Host publication. Adapters own HTTP, private storage and Relay transport.
mod controller;
mod ports;
mod types;

pub use controller::Controller;
pub use ports::{Center, CredentialStore, ManagedRelay};
pub use types::{Binding, Credential, Error, Machine, Pending, Session, State, Tokens};

/// The only production authority for unattended machines.
pub const CENTER: &str = "https://dash.ait-app.com:8443/api";
/// The browser destination for human approval; never supplied by a token.
pub const VERIFICATION_URI: &str = "https://dash.ait-app.com:8443/auth/device";
