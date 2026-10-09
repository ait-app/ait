//! Structured wording generation shared by metadata, filesystem and provider use cases.

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;

use domain::summary::{SummaryError, SummaryRequest};
use serde_json::Value;

/// Sendable generation future with validated JSON output for the requested artifact.
pub type SummaryFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, SummaryError>> + Send + 'a>>;

/// Consumer boundary for host-supplied summaries; provider owns the generation capability.
pub trait SummarySource: Debug + Send + Sync {
    /// Generate the artifact in `request`, or return a safe budget/provider failure.
    /// # Errors
    /// Returns unavailable output, exhausted admission, or shutdown cancellation.
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_>;

    /// Cancel outstanding auxiliary work when the server drains. Does not cancel user turns.
    fn shutdown(&self);
}

/// Isolated summary generation; provider implements it and owns budgets and native cleanup.
pub trait SummaryGenerator: Debug + Send + Sync {
    /// Generate and validate the summary requested by `request`.
    /// # Errors
    /// Returns unavailable output, admission exhaustion, or shutdown cancellation.
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_>;

    /// Cancel auxiliary operations when the daemon drains, preserving foreground turns.
    fn shutdown(&self);
}

/// Preferences consumed by summary generation, independent of their persistence owner.
/// Blocking reads must run outside an async reactor.
pub trait SummaryConfiguration: Debug + Send + Sync {
    /// Read live provider/model preferences.
    /// # Errors
    /// Returns unavailable when configuration cannot be read.
    fn current(&self) -> Result<Value, SummaryError>;

    /// Read project wording preferences for `cwd`; missing preferences use defaults.
    fn project(&self, cwd: &str) -> Value;
}
