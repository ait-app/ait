//! Filesystem, Git, Forge and repository provisioning capabilities.
//! Blocking services and adapters; the host owns connections and task scheduling.

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
