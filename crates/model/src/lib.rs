//! Shared collaboration ports, server transport definitions and runtime resources.

/// Build identity used by the daemon and its public protocols.
pub const BUILD_VERSION: &str = env!("AIT_BUILD_VERSION");

pub mod bot;
pub mod changes;
mod context;
pub mod creation;
pub mod directory_sync;
pub mod events;
pub mod hub;
pub mod methods;
pub mod outbound;
pub mod pagination;
pub mod polling;
pub mod realtime;
pub mod rpc;
pub mod runtime;
pub mod server;
pub mod session;
pub mod storage;
pub mod summary;
pub mod workspace;

pub use context::{Context, DispatchError, Request};
pub use runtime::{LifecycleIntent, Runtime};
pub use server::{ErrorCode, Lifecycle, Limits, ServerInfo, ServerMessage, VERSION, valid_id};

#[cfg(test)]
mod tests;
