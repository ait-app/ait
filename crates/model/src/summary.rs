//! Structured wording generation shared by metadata, filesystem and provider use cases.

use std::fmt::Debug;

use domain::summary::{SummaryError, SummaryRequest};
use serde_json::Value;

/// Consumer boundary for host-supplied summaries; provider owns the generation capability.
#[async_trait::async_trait]
pub trait SummarySource: Debug + Send + Sync {
    /// Generate the artifact in `request`, or return a safe budget/provider failure.
    /// # Errors
    /// Returns unavailable output, exhausted admission, or shutdown cancellation.
    async fn generate(&self, request: SummaryRequest) -> Result<Value, SummaryError>;

    /// Cancel outstanding auxiliary work when the server drains. Does not cancel user turns.
    fn shutdown(&self);
}

/// Isolated summary generation; provider implements it and owns budgets and native cleanup.
#[async_trait::async_trait]
pub trait SummaryGenerator: Debug + Send + Sync {
    /// Generate and validate the summary requested by `request`.
    /// # Errors
    /// Returns unavailable output, admission exhaustion, or shutdown cancellation.
    async fn generate(&self, request: SummaryRequest) -> Result<Value, SummaryError>;

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
