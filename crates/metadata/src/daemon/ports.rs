//! Host-owned adapter boundaries.
//!
//! Host-owned evidence collection boundary.

pub mod host_monitor;

/// Bounded and sanitized local incident evidence supplied by the host.
pub trait DaemonDiagnostics: Send + Sync + std::fmt::Debug {
    /// Collect a report on a blocking worker, including any partial failures in the text.
    fn report(&self) -> String;
}
