//! Bonsai Runtime adapter: an outbound runtime that runs Bonsai-dispatched work on local agents.
//!
//! The crate connects to the user's own Bonsai (`wss://<host>/runtime`), accepts
//! `run.dispatch`, starts Agent sessions through host-supplied ports, translates each session
//! into neutral `bonsai.session/1` events, and relays member input, interrupts, approvals and
//! cancellation. It depends only on `model`; the host implements the ports.

pub mod config;
pub mod event;
pub mod hello;
pub mod link;
pub mod outbox;
pub mod ports;
pub mod prompt;
pub(crate) mod runs;
pub mod service;
mod session;
pub mod settings;
pub mod store;
pub mod translate;
pub mod wire;

// Enables ring for the WebSocket TLS backend; the crate calls no rustls API itself.
use rustls as _;

#[cfg(test)]
mod testing;

pub use config::{Config, ConfigError};
pub use service::{Service, StartError};
