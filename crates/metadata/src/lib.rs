//! Project/Workspace metadata and automation: wire types, services and local scripts.
//!
//! The host owns transports and task scheduling; model owns shared contracts and file owns persistence.

mod installation;

/// Complete crate-level service and its required composition inputs.
pub use installation::{Dependencies, Service};

pub mod capabilities;
pub mod dispatch;
pub mod local;
pub mod ports;
pub mod protocol;
pub mod rpc;
pub mod service;

/// Connection-owned observers and request integration.
pub mod connection;
