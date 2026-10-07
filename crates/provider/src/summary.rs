//! Provider-owned auxiliary summary generation capability.

use std::fmt::Debug;

use model::summary::{SummaryError, SummaryFuture, SummaryRequest};
use serde_json::Value;

/// Isolated summary generation; implementations own budgets and native cleanup.
pub trait SummaryGenerator: Debug + Send + Sync {
    /// Generate and validate the summary requested by `request`.
    /// # Errors
    /// Returns unavailable output, admission exhaustion, or shutdown cancellation.
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_>;
    /// Cancel auxiliary operations when the daemon drains, preserving foreground turns.
    fn shutdown(&self);
}

/// Configuration consumed by summary generation, independent of its persistence owner.
pub trait SummaryConfiguration: Debug + Send + Sync {
    /// Read live provider/model preferences.
    /// # Errors
    /// Returns unavailable when configuration cannot be read.
    fn current(&self) -> Result<Value, SummaryError>;
    /// Read project wording preferences for `cwd`; missing preferences use defaults.
    fn project(&self, cwd: &str) -> Value;
}
