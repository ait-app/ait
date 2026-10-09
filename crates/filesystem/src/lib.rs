//! Filesystem, Git, Forge and repository provisioning capabilities.
//! Blocking services and adapters; the host owns connections and task scheduling.
//!
//! Capability groups (`git`, `forge`, `worktrees`, `files`, `skills`) each keep the
//! `ports`/`protocol`/`service`/`rpc`/`connection`/`local` layering. Groups reference each
//! other only through `ports` and `protocol`; the top-level modules compose concrete types.

mod installation;

/// Complete crate-level service and its required composition inputs.
pub use installation::{Dependencies, Service};

pub mod capabilities;
/// Connection-owned observers and request integration.
pub mod connection;
pub mod dispatch;
pub mod workspace_runtime;

pub mod files;
pub mod forge;
pub mod git;
pub mod skills;
pub mod worktrees;

mod support;
