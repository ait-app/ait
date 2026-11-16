//! Contracts shared by the provider runtime and the native adapters.
//!
//! Adapters implement these ports; the runtime drives them. Neither side depends on the other.

pub mod ports;
pub mod protocol;
