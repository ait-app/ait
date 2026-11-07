//! Verified OMP native host runtime resolution for host binaries.
//!
//! The implementation lives with the OMP adapter because it shares that adapter's manifest
//! schema; this module only re-exports the resolution surface so a daemon can report the
//! runtime it will use without depending on adapter internals.

pub use crate::local::omp::native_host::{NativeHost, discover};
