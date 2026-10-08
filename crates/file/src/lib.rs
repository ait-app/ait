//! Single-file I/O, observations, registries, startup configuration, and persistence adapters.
//!
//! Blocking reads, writes, and registry operations belong outside an async reactor.
//! Hosts retain ownership of data-directory leases and application coordination.

pub mod config;
pub mod creation;
/// Bounded local incident file persistence and retention.
pub mod diagnostics;
pub mod registry;
mod single;
pub mod storage;
pub mod watch;

pub use single::{Error, File};
