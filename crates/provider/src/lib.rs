//! Agent configuration, durable runtime directory and native Provider session coordination.
//!
//! Pure Agent values remain in `domain`; HTTP/WS transports belong to the host. This crate owns the bounded native Provider worker.

mod installation;

/// Complete crate-level service and its required composition inputs.
pub use installation::{Dependencies, Service};

pub mod capabilities;
mod composition;
pub mod dispatch;
mod local;
pub mod ports;
pub mod protocol;
pub mod rpc;
pub mod service;
pub mod storage;
pub mod summary;

pub use composition::Providers;
pub use summary::{SummaryConfiguration, SummaryGenerator};

#[cfg(all(test, unix))]
mod test_support;

/// Provider-owned physical connection observers.
pub mod connection;
