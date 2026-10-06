//! Agent configuration, durable runtime directory and native Provider session coordination.
//!
//! Pure Agent values remain in `domain`; HTTP/WS transports belong to the host. This crate owns the bounded native Provider worker.

pub mod capabilities;
mod composition;
pub mod dispatch;
mod local;
pub mod ports;
pub mod protocol;
pub mod rpc;
pub mod service;
pub mod storage;

pub use composition::Providers;

#[cfg(all(test, unix))]
mod test_support;

/// Provider-owned physical connection observers.
pub mod connection;
