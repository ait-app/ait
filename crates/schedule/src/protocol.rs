//! Schedule input and shared durable records.

use chrono::{DateTime, Utc};
use domain::schedule::{Cadence, Target};
use serde::Deserialize;

/// Input accepted by create; unknown fields are rejected before any write.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Create {
    /// Optional display name.
    pub name: Option<String>,
    /// Nonempty prompt.
    pub prompt: String,
    /// Schedule cadence.
    pub cadence: Cadence,
    /// Existing Agent or new-Agent configuration.
    pub target: Target,
    /// Completed-run limit.
    pub max_runs: Option<u64>,
    /// Automatic expiry.
    pub expires_at: Option<DateTime<Utc>>,
    /// Defaults to true for intervals and false for cron.
    pub run_on_create: Option<bool>,
}
