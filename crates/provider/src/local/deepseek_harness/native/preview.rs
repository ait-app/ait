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

/// One response budget shared by display-window replay and activity discovery.
#[derive(Default)]
struct PageBudget {
    pages: usize,
    bytes: usize,
}

impl PageBudget {
    /// Read one page at the fixed cut, accounting for its full serialized response.
    async fn page(
        &mut self,
        runtime: &Runtime,
        address: &Value,
        cut: i64,
        before: i64,
    ) -> Result<Value, AgentSessionError> {
        if self.pages >= MAX_WIRE_PAGES {
            return Err(AgentSessionError::Failed);
        }
        self.pages += 1;
        let mut request = json!({"request":{"address":address,"throughSeq":cut,"maxMessages":WIRE_PAGE_MESSAGES}});
        if before <= cut {
            request["request"]["beforeSeq"] = json!(before);
        }
        let page = runtime.api.call("session/page", request).await?;
        // Count without allocating a second serialized copy of the native page.
        serde_json::to_writer(self, &page).map_err(|_| AgentSessionError::Failed)?;
        Ok(page)
    }
}

impl std::io::Write for PageBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_PAGE_BYTES)
            .ok_or_else(|| std::io::Error::other("Native preview record budget exceeded"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

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
    let mut budget = PageBudget::default();
    let mut has_more = false;
    for _ in 0..MAX_WIRE_PAGES {
        let page = budget.page(runtime, &address, cut, before).await?;
        let page_records = validated_records(&page, before)?;
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
        let mut window = page_records.to_vec();
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
    let active = if initial_before == cut + 1 {
        match latest_turn_boundary(&records) {
            Some(active) => active,
            None => activity_before(runtime, &address, cut, before, &mut budget).await?,
        }
    } else {
        // An older display window cannot establish activity at the fixed newest cut.
        activity_before(runtime, &address, cut, cut + 1, &mut budget).await?
    };
    let (entries, older_cursor) =
        display_page(entries, request, cut, before..initial_before, has_more)?;
    Ok(SessionPreview {
        descriptor: descriptor(&summary, &values, request),
        active,
        entries,
        older_cursor,
    })
}

/// Find the latest recorded turn boundary in an ascending native sequence window.
fn latest_turn_boundary(records: &[Value]) -> Option<bool> {
    records
        .iter()
        .rev()
        .find_map(|record| match record["event"]["type"].as_str() {
            Some("turn/start") => Some(true),
            Some("turn/end") => Some(false),
            _ => None,
        })
}

/// Resolve activity at the fixed cut; an exhausted budget is unknown, never inactive.
async fn activity_before(
    runtime: &Runtime,
    address: &Value,
    cut: i64,
    mut before: i64,
    budget: &mut PageBudget,
) -> Result<bool, AgentSessionError> {
    while before > 0 {
        let page = budget.page(runtime, address, cut, before).await?;
        let records = validated_records(&page, before)?;
        let has_more = page["hasMore"].as_bool().ok_or(AgentSessionError::Failed)?;
        let first = records.first().ok_or(AgentSessionError::Failed)?;
        before = first["event"]["seq"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?;
        if let Some(active) = latest_turn_boundary(records) {
            return Ok(active);
        }
        if !has_more && before != 0 {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(false)
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
fn validated_records(page: &Value, before: i64) -> Result<&[Value], AgentSessionError> {
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
    Ok(records)
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
