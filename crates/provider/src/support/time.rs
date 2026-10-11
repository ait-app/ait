//! Timestamps shared by provider services; the wire format is fixed here once.

use chrono::{SecondsFormat, Utc};

/// Format the current instant as a millisecond-precision UTC RFC 3339 timestamp.
///
/// # Returns
/// The current time with milliseconds and a `Z` suffix, matching persisted Agent
/// runtime timestamps.
pub(crate) fn now_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
