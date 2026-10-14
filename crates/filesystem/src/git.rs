//! Git checkout, background fetch, directory provisioning and managed worktree adapters.

pub(crate) mod connection;
pub mod local;
pub(crate) mod ports;
pub(crate) mod protocol;
pub mod rpc;
pub mod service;
