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
