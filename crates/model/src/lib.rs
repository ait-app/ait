//! Shared collaboration ports, server transport definitions and runtime resources.

pub mod changes;
mod context;
pub mod creation;
pub mod directory_sync;
pub mod events;
pub mod methods;
pub mod outbound;
pub mod pagination;
pub mod polling;
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
