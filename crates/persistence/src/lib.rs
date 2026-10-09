//! Single-file I/O, registries, and concrete persistence adapters.
//!
//! Blocking reads, writes, and registry operations belong outside an async reactor.
//! Hosts retain ownership of data-directory leases, startup configuration, and application
//! coordination.

mod registry;
mod single;
pub mod storage;

pub use single::{Error, File};
