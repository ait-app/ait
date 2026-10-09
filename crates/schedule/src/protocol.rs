//! Schedule input and shared durable records.

use chrono::{DateTime, Utc};
use domain::schedule::{Cadence, Target};
use serde::Deserialize;

/// Input accepted by create; unknown fields are rejected before any write.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(clippy::struct_field_names, reason = "field names are the wire schema")]
pub(crate) struct Create {
    /// Optional display name.
    pub(crate) name: Option<String>,
    /// Nonempty prompt.
    pub(crate) prompt: String,
    /// Schedule cadence.
    pub(crate) cadence: Cadence,
    /// Existing Agent or new-Agent configuration.
    pub(crate) target: Target,
    /// Completed-run limit.
    pub(crate) max_runs: Option<u64>,
    /// Automatic expiry.
    pub(crate) expires_at: Option<DateTime<Utc>>,
    /// Defaults to true for intervals and false for cron.
    pub(crate) run_on_create: Option<bool>,
}
