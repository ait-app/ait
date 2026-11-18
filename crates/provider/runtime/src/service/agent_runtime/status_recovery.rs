//! Startup recovery of Agent status that deferred writes may have lost (ADR-288).
//!
//! Status updates reach `agents.json` up to one flush interval late. After a crash the file
//! holds the state as of its `lastChangedAt`; user input recorded in the timeline around and
//! after that instant restores the input timestamps. Usage and runtime details are refreshed by
//! providers and are not rebuilt.

use chrono::{DateTime, SecondsFormat, TimeDelta, Utc};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::ErrorCode;

use crate::storage::timeline::Timeline;
use crate::storage::timeline::activity::parse_timestamp;

/// How far before `lastChangedAt` recovery also looks. A user message is stamped when it is
/// recorded, but its deferred input write commits on a later drain; another Agent's write in
/// between moves `lastChangedAt` past the message. Recovery only moves timestamps forward, so
/// a wide margin costs a few extra rows, never a wrong value.
const MARGIN: TimeDelta = TimeDelta::minutes(10);

/// Restore input timestamps from timeline user messages newer than [`MARGIN`] before `since`.
///
/// # Arguments
///
/// * `registry` - Agent records loaded from `agents.json`
/// * `timeline` - Timeline holding user messages committed before the crash
/// * `since` - The `lastChangedAt` of a file that was not closed, an RFC 3339 timestamp
///
/// # Returns
///
/// The number of Agents whose timestamps moved forward. Changes are deferred status updates;
/// the caller flushes the registry once afterwards.
///
/// # Errors
///
/// Returns `InvalidMessage` for an unparseable `since`, and `AgentIo` when the registry or the
/// timeline cannot be read or updated.
pub fn recover_status(
    registry: &dyn AgentRuntimeRegistry,
    timeline: &Timeline,
    since: &str,
) -> Result<usize, ErrorCode> {
    let cutoff = parse_timestamp(since).ok_or(ErrorCode::InvalidMessage)? - MARGIN;
    let mut recovered = 0;
    for record in registry.snapshot().map_err(|_| ErrorCode::AgentIo)?.iter() {
        let Some(at) = timeline.latest_user_message_after(&record.id, cutoff)? else {
            continue;
        };
        if !is_later(record.last_user_message_at.as_deref(), at) {
            continue;
        }
        let timestamp = at.to_rfc3339_opts(SecondsFormat::Millis, true);
        registry
            .update_status(&record.id, &|current| {
                let mut next = current.clone();
                next.last_user_message_at = Some(timestamp.clone());
                if is_later(next.last_activity_at.as_deref(), at) {
                    next.last_activity_at = Some(timestamp.clone());
                }
                if is_later(Some(&next.updated_at), at) {
                    next.updated_at.clone_from(&timestamp);
                }
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?;
        recovered += 1;
    }
    Ok(recovered)
}

/// Whether `candidate` is later than `current`; an absent or unreadable `current` is earlier.
fn is_later(current: Option<&str>, candidate: DateTime<Utc>) -> bool {
    current
        .and_then(parse_timestamp)
        .is_none_or(|known| candidate > known)
}

#[cfg(test)]
mod tests;
