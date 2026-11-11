//! Read-only paged preview of one native session through non-activating endpoints.
//!
//! Every page reads the same fixed native cut: `session/list` validates the root identity,
//! `session/projections` supplies the authoritative cut for a fresh preview, and
//! `session/page` walks backwards from it. No `session/follow`, create, prompt or model
//! selection is ever issued, so browsing never promotes a cold session to an Agent.

use serde_json::{Value, json};

use super::super::{DeepSeekHarnessClient, PROVIDER};
use super::{history, native_sessions, runtime::Runtime};
use crate::ports::agent_session::AgentSessionError;
use crate::ports::native_history::{
    PreviewCursor, PreviewRequest, PreviewWindow, SessionDescriptor, SessionPreview,
};
use crate::protocol::timeline::NativeItem;

/// Largest display-item count accepted for one preview page.
const MAX_PAGE_ITEMS: usize = 100;
/// Native records requested per wire page; projection may collapse several into one item.
const WIRE_PAGE_MESSAGES: usize = 50;
/// Wire-page ceiling for one preview response, bounding cold reads of sparse projections.
const MAX_WIRE_PAGES: usize = 16;
/// Serialized record budget for one preview response.
const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;

/// Read one bounded newest-first preview page without creating or restoring a writer.
/// Rejects unsupported transports, invalid limits, foreign identities and stale cursors.
pub(in crate::local::deepseek_harness) async fn read(
    client: &DeepSeekHarnessClient,
    request: &PreviewRequest,
) -> Result<SessionPreview, AgentSessionError> {
    if !client.interactive {
        return Err(AgentSessionError::Unavailable);
    }
    if request.provider != PROVIDER
        || !(1..=MAX_PAGE_ITEMS).contains(&request.limit)
        || !std::path::Path::new(&request.cwd).is_absolute()
    {
        return Err(AgentSessionError::Rejected);
    }
    native_sessions::validate_id(&request.session_id)?;
    if let Some(cursor) = &request.cursor {
        validate_cursor(cursor, request)?;
    }
    let mut runtime = Runtime::open(client, &request.cwd).await?;
    let result = read_page(&mut runtime, client, request).await;
    let closed = runtime.close().await;
    let preview = result?;
    closed?;
    Ok(preview)
}

/// Reject cursors that do not belong to this exact request identity and fixed cut.
fn validate_cursor(
    cursor: &PreviewCursor,
    request: &PreviewRequest,
) -> Result<(), AgentSessionError> {
    if cursor.provider != request.provider
        || cursor.provider_handle_id != request.session_id
        || cursor.cwd != request.cwd
        || cursor.through_seq < -1
        || cursor.through_seq == i64::MAX
        || cursor.before_seq < 0
        || cursor.before_seq > cursor.through_seq + 1
        || cursor.window.as_ref().is_some_and(|window| {
            window.start_seq < 0
                || window.start_seq >= cursor.before_seq
                || window.skipped_items == 0
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

async fn read_page(
    runtime: &mut Runtime,
    client: &DeepSeekHarnessClient,
    request: &PreviewRequest,
) -> Result<SessionPreview, AgentSessionError> {
    let summary = root_summary(runtime, &request.session_id, &request.cwd).await?;
    let (cut, values) = match &request.cursor {
        Some(cursor) => (cursor.through_seq, Value::Null),
        None => history::baseline(runtime, &request.session_id).await?,
    };
    let address = json!({"kind":"session","sessionId":request.session_id});
    let mut before = request
        .cursor
        .as_ref()
        .map_or(cut.checked_add(1), |cursor| Some(cursor.before_seq))
        .ok_or(AgentSessionError::Failed)?;
    let initial_before = before;
    let replay = request
        .cursor
        .as_ref()
        .and_then(|cursor| cursor.window.as_ref());
    let mut records = Vec::new();
    let mut entries = Vec::new();
    let mut bytes = 0usize;
    let mut has_more = false;
    for _ in 0..MAX_WIRE_PAGES {
        let mut wire_request = json!({"request":{"address":address,"throughSeq":cut,"maxMessages":WIRE_PAGE_MESSAGES}});
        if before <= cut {
            wire_request["request"]["beforeSeq"] = json!(before);
        }
        let page = runtime.api.call("session/page", wire_request).await?;
        let page_records = validated_records(&page, before)?;
        bytes = bytes
            .checked_add(page.to_string().len())
            .ok_or(AgentSessionError::Failed)?;
        if bytes > MAX_PAGE_BYTES {
            return Err(AgentSessionError::Failed);
        }
        has_more = page["hasMore"].as_bool().ok_or(AgentSessionError::Failed)?;
        if page_records.is_empty() {
            // The first page of an empty journal legitimately ends at the native zero boundary.
            if has_more || (before != 0 && before != cut + 1) {
                return Err(AgentSessionError::Failed);
            }
            break;
        }
        before = page_records[0]["event"]["seq"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?;
        // Wire pages arrive newest-first; keep that order across the accumulated window.
        let mut window = page_records;
        window.extend(records);
        records = window;
        if let Some(replay) = replay {
            if before > replay.start_seq {
                if !has_more {
                    return Err(AgentSessionError::Failed);
                }
                continue;
            }
            // A native page may straddle the saved window boundary. Project only that
            // exact window so deduplication cannot shift the already-returned item offset.
            records.retain(|record| {
                record["event"]["seq"]
                    .as_i64()
                    .is_some_and(|seq| seq >= replay.start_seq)
            });
            before = replay.start_seq;
            has_more = before > 0;
        }
        entries =
            history::project_records(runtime, client, &request.session_id, records.clone()).await?;
        if replay.is_some() || entries.len() >= request.limit || !has_more {
            break;
        }
        if before == 0 {
            has_more = false;
            break;
        }
    }
    if replay.is_some_and(|window| before != window.start_seq) {
        return Err(AgentSessionError::Failed);
    }
    let active = records
        .iter()
        .rev()
        .find(|record| {
            matches!(
                record["event"]["type"].as_str(),
                Some("turn/start" | "turn/end")
            )
        })
        .is_some_and(|record| record["event"]["type"] == "turn/start");
    let (entries, older_cursor) =
        display_page(entries, request, cut, before..initial_before, has_more)?;
    Ok(SessionPreview {
        descriptor: descriptor(&summary, &values, request),
        active,
        entries,
        older_cursor,
    })
}

/// Keep excess display items in a bounded replay cursor, including on the final native page.
fn display_page(
    mut entries: Vec<NativeItem>,
    request: &PreviewRequest,
    cut: i64,
    bounds: std::ops::Range<i64>,
    has_more: bool,
) -> Result<(Vec<NativeItem>, Option<PreviewCursor>), AgentSessionError> {
    entries.reverse();
    let skipped = request
        .cursor
        .as_ref()
        .and_then(|cursor| cursor.window.as_ref())
        .map_or(0, |window| window.skipped_items);
    if skipped > 0 {
        if skipped >= entries.len() {
            return Err(AgentSessionError::Rejected);
        }
        entries.drain(..skipped);
    }
    let window = if entries.len() > request.limit {
        Some(PreviewWindow {
            start_seq: bounds.start,
            skipped_items: skipped
                .checked_add(request.limit)
                .ok_or(AgentSessionError::Failed)?,
        })
    } else {
        None
    };
    let older_cursor =
        (window.is_some() || (has_more && bounds.start > 0)).then(|| PreviewCursor {
            provider: request.provider.clone(),
            provider_handle_id: request.session_id.clone(),
            cwd: request.cwd.clone(),
            through_seq: cut,
            before_seq: if window.is_some() {
                bounds.end
            } else {
                bounds.start
            },
            window,
        });
    entries.truncate(request.limit);
    Ok((entries, older_cursor))
}

/// Find the listed root summary and validate its directory and origin before any page read.
async fn root_summary(
    runtime: &Runtime,
    session_id: &str,
    cwd: &str,
) -> Result<Value, AgentSessionError> {
    let listed = runtime
        .api
        .call("session/list", json!({"_request":{}}))
        .await?;
    let summary = listed["items"]
        .as_array()
        .and_then(|items| items.iter().find(|item| item["sessionId"] == session_id))
        .ok_or(AgentSessionError::Failed)?;
    if summary["cwd"] != cwd || summary["origin"] == "subagent" {
        return Err(AgentSessionError::Failed);
    }
    Ok(summary.clone())
}

/// Validate one wire page: event records, contiguous sequences below the exclusive boundary.
fn validated_records(page: &Value, before: i64) -> Result<Vec<Value>, AgentSessionError> {
    let records = page["records"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut previous = None;
    for record in records {
        let seq = record["event"]["seq"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?;
        if record["type"] != "event"
            || seq < 0
            || seq >= before
            || previous.is_some_and(|last| seq != last + 1)
        {
            return Err(AgentSessionError::Failed);
        }
        previous = Some(seq);
    }
    if previous.is_some_and(|last| last + 1 != before) {
        return Err(AgentSessionError::Failed);
    }
    Ok(records.clone())
}

/// Build display metadata from validated native facts; cached continuation pages keep the
/// listed title and activity instead of re-reading projections.
fn descriptor(summary: &Value, values: &Value, request: &PreviewRequest) -> SessionDescriptor {
    let updated = summary["updatedAt"].as_i64().unwrap_or_default();
    let last_activity_at = match chrono::DateTime::from_timestamp_millis(updated) {
        Some(value) => value.to_rfc3339(),
        None => crate::support::time::now_timestamp(),
    };
    SessionDescriptor {
        provider_id: PROVIDER.into(),
        provider_label: "DeepSeek Harness".into(),
        provider_handle_id: request.session_id.clone(),
        cwd: request.cwd.clone(),
        title: values["title"]
            .as_str()
            .or_else(|| summary["projections"]["values"]["title"].as_str())
            .map(str::to_owned),
        first_prompt_preview: None,
        last_prompt_preview: None,
        last_activity_at,
    }
}
