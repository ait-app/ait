//! Center-managed Host publication. Adapters own HTTP, private storage and Hub transport.
pub mod authorization;
mod controller;
mod ports;
mod types;

pub use controller::Controller;
pub use ports::{Center, CredentialStore, ManagedHub};
pub use types::{Binding, Credential, Error, Machine, Pending, Session, State, Tokens};

/// The only production authority for unattended machines.
pub const CENTER: &str = "https://aitapp.cn/api";
/// The browser destination for human approval; never supplied by a token.
pub const VERIFICATION_URI: &str = "https://aitapp.cn/auth/device";
