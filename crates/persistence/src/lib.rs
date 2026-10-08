//! Single-file I/O, observations, registries, and concrete persistence adapters.
//!
//! Blocking reads, writes, and registry operations belong outside an async reactor.
//! Hosts retain ownership of data-directory leases, startup configuration, and application
//! coordination.

pub mod registry;
mod single;
pub mod storage;
pub mod watch;

pub use single::{Error, File};
