//! Single-file I/O, observations, registries, startup configuration, and persistence adapters.
//!
//! Blocking reads, writes, and registry operations belong outside an async reactor.
//! Hosts retain ownership of data-directory leases and application coordination.

pub mod config;
pub mod creation;
pub mod registry;
mod single;
pub mod storage;
pub mod watch;

pub use single::{Error, File};
