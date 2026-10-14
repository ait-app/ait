//! Project/Workspace metadata and automation: wire types, services and local scripts.
//!
//! The host owns transports and task scheduling; model owns shared contracts and persistence
//! owns concrete storage.
//!
//! Capability groups (`server`, `daemon`, `push`, `directory`, `labels`, `automation`,
//! `worktrees`) each keep the `ports`/`protocol`/`service`/`rpc`/`connection`/`local` layering.
//! Groups reference each other only through `ports` and `protocol`; the top-level modules
//! compose concrete types.

mod installation;

/// Complete crate-level service and its required composition inputs.
pub use installation::{Dependencies, Service};

pub mod capabilities;
pub mod collaboration;
/// Connection-owned observers and request integration.
pub mod connection;
pub mod dispatch;

pub mod automation;
pub mod daemon;
pub mod directory;
pub mod labels;
pub mod push;
pub mod server;
pub mod worktrees;

pub mod support;
