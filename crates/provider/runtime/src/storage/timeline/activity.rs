//! Read-only activity facts for rebuilding deferred Agent status after a crash (ADR-288).

use chrono::{DateTime, Utc};
use model::ErrorCode;

use super::{Timeline, io};

/// Rows read per Agent at most, so plugin rows or unparseable timestamps cannot force a scan of
/// the whole history at startup.
const MAX_SCANNED_ROWS: usize = 4096;

const NEWEST_FIRST: &str =
    "SELECT json_extract(entry,'$.item.type'), json_extract(entry,'$.timestamp')
     FROM entries WHERE agent=?1 ORDER BY seq DESC LIMIT ?2";

impl Timeline {
    /// Find the newest user message for `agent` that is later than `since`.
    ///
    /// Rows are read newest first and the scan stops at the first row at or before `since`, so
    /// the cost follows the activity after `since`, not the history length. Plugin rows are
    /// skipped because rebuilds re-append them with their original timestamps. At most
    /// [`MAX_SCANNED_ROWS`] rows are read.
    ///
    /// # Returns
    /// The message's timestamp, or `None` when no user message is later than `since`.
    ///
    /// # Errors
    /// Returns `AgentIo` when the timeline cannot be read.
    pub(crate) fn latest_user_message_after(
        &self,
        agent: &str,
        since: DateTime<Utc>,
    ) -> Result<Option<DateTime<Utc>>, ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        let mut statement = database.prepare_cached(NEWEST_FIRST).map_err(io)?;
        let mut rows = statement
            .query(rusqlite::params![agent, MAX_SCANNED_ROWS])
            .map_err(io)?;
        let mut scanned = 0_usize;
        while let Some(row) = rows.next().map_err(io)? {
            scanned += 1;
            let kind: Option<String> = row.get(0).map_err(io)?;
            if kind.as_deref() == Some("plugin") {
                continue;
            }
            let timestamp: Option<String> = row.get(1).map_err(io)?;
            let Some(at) = timestamp.as_deref().and_then(parse_timestamp) else {
                continue;
            };
            if at <= since {
                return Ok(None);
            }
            if kind.as_deref() == Some("user_message") {
                return Ok(Some(at));
            }
        }
        if scanned == MAX_SCANNED_ROWS {
            tracing::debug!(
                agent_id = agent,
                "Status recovery stopped at the row limit before its cutoff"
            );
        }
        Ok(None)
    }
}

/// Parse an RFC 3339 timestamp in any offset as UTC.
pub(crate) fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

#[cfg(test)]
mod tests;
