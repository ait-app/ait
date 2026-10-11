//! Host-owned evidence collection boundary.

/// Bounded and sanitized local incident evidence supplied by the host.
pub trait DaemonDiagnostics: Send + Sync + std::fmt::Debug {
    /// Collect a report on a blocking worker, including any partial failures in the text.
    fn report(&self) -> String;
}
