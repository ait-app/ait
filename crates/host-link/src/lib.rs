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
/// The browser destination for human approval; never supplied by a token.
pub const VERIFICATION_URI: &str = "https://aitapp.cn/auth/device";
