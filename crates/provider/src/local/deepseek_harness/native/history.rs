//! Read a fixed native journal cut without creating a session or sending a prompt.
use super::super::{DeepSeekHarnessClient, PROVIDER};
use super::{content, projection, runtime::Runtime};
use crate::local::acp_streaming::Stream;
use crate::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};
use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A fixed journal cut and its native metadata, shared by replay and import.
pub(super) struct Journal {
    pub(super) parent_id: Option<String>,
    pub(super) created_at: String,
    pub(super) values: Value,
    pub(super) entries: Vec<NativeItem>,
    pub(super) active: bool,
}

/// Recover display entries for a registered native handle and its exact working directory.
/// Returns an ordered projection, or an error for invalid handles, transport failures,
/// incomplete journals, or content that cannot be projected. Never submits input.
pub(in crate::local::deepseek_harness) async fn read(
    client: &DeepSeekHarnessClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    if handle.provider != PROVIDER
        || handle.session_id.is_empty()
        || !std::path::Path::new(cwd).is_absolute()
        || handle
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("cwd"))
            .and_then(Value::as_str)
            != Some(cwd)
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut runtime = Runtime::open(client, cwd).await?;
    let result = read_snapshot(&mut runtime, client, &handle.session_id, cwd).await;
    let closed = runtime.close().await;
    let result = result?;
    closed?;
    Ok(result.entries)
}

/// Read a complete, fixed history cut without activating its native Agent.
/// Rejects mismatched identities, subagents, and incomplete journals.
pub(super) async fn read_snapshot(
    runtime: &mut Runtime,
    client: &DeepSeekHarnessClient,
    session_id: &str,
    cwd: &str,
) -> Result<Journal, AgentSessionError> {
    let address = json!({"kind":"session","sessionId":session_id});
    // The Web client's follow endpoint promotes a cold session to an Agent after its snapshot.
    // Only these non-activating endpoints may be used by history/import readers.
    let parent_id = validate_root(runtime, session_id, cwd).await?;
    // The read-only wire summary omits creation time. The shared import contract permits the
    // observation time in this case; do not fabricate a native header from an event timestamp.
    let created_at = chrono::Utc::now().to_rfc3339();
    let (cut, values) = baseline(runtime, session_id).await?;
    let page = runtime
        .api
        .call(
            "session/page",
            json!({"request":{"address":address,"throughSeq":cut,"maxMessages":50}}),
        )
        .await?;
    let records = pages(runtime, &address, cut, page).await?;
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
    let entries = project_records(runtime, client, session_id, records).await?;
    Ok(Journal {
        parent_id,
        created_at,
        values,
        entries,
        active,
    })
}

/// Project ordered native records into deduplicated display items without submitting input.
/// Image hydration uses the native read-only attachment endpoint; no writer is addressed.
pub(super) async fn project_records(
    runtime: &Runtime,
    client: &DeepSeekHarnessClient,
    session_id: &str,
    records: Vec<Value>,
) -> Result<Vec<NativeItem>, AgentSessionError> {
    let mut stream = Stream::new(client.images.clone());
    let mut tools = BTreeMap::new();
    let mut entries: Vec<NativeItem> = Vec::new();
    let mut positions = BTreeMap::new();
    for record in records {
        let mut frame = json!({"streamId":"history","value":record});
        if content::has_images(&frame) {
            frame = content::hydrate(frame, &runtime.api, session_id, &client.images).await?;
        }
        projection::apply(
            &mut stream,
            &mut tools,
            &frame["value"]["event"],
            session_id,
        )?;
        for event in stream.events.drain(..) {
            let (AgentTurnEvent::Timeline(entry) | AgentTurnEvent::Progress { entry, .. }) = event
            else {
                continue;
            };
            if let Some(index) = positions.get(&entry.key) {
                entries[*index] = entry;
            } else {
                positions.insert(entry.key.clone(), entries.len());
                entries.push(entry);
            }
        }
    }
    Ok(entries)
}

/// Validate a root identity and working directory before any activating native subscription.
/// Returns its optional native parent, or an error for absent, foreign, or subagent identities.
pub(super) async fn validate_root(
    runtime: &Runtime,
    session_id: &str,
    cwd: &str,
) -> Result<Option<String>, AgentSessionError> {
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
    Ok(summary["parentSessionId"].as_str().map(str::to_owned))
}

/// Obtain an exact read-only projection cut; migration hints cannot replace authoritative facts.
/// Missing endpoints and invalid baselines fail without falling back to an activating follow.
pub(super) async fn baseline(
    runtime: &Runtime,
    session_id: &str,
) -> Result<(i64, Value), AgentSessionError> {
    let mut value = runtime
        .api
        .call(
            "session/projections",
            json!({"request":{"sessionId":session_id}}),
        )
        .await?;
    let cut = value["asOfSeq"]
        .as_i64()
        .filter(|cut| *cut >= -1 && *cut < i64::MAX)
        .ok_or(AgentSessionError::Failed)?;
    if value["kind"] != "sequenced" || !value["values"].is_object() {
        return Err(AgentSessionError::Failed);
    }
    Ok((cut, value["values"].take()))
}

async fn pages(
    runtime: &Runtime,
    address: &Value,
    cut: i64,
    mut page: Value,
) -> Result<Vec<Value>, AgentSessionError> {
    let mut pages = Vec::new();
    let mut before = cut.checked_add(1).ok_or(AgentSessionError::Failed)?;
    let mut total = 0usize;
    loop {
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
        total = total
            .checked_add(page.to_string().len())
            .ok_or(AgentSessionError::Failed)?;
        if total > 64 * 1024 * 1024 || pages.len() >= 4096 {
            return Err(AgentSessionError::Failed);
        }
        let has_more = page["hasMore"].as_bool().ok_or(AgentSessionError::Failed)?;
        if records.is_empty() {
            if has_more || before != 0 {
                return Err(AgentSessionError::Failed);
            }
            break;
        }
        before = records[0]["event"]["seq"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?;
        pages.push(records.clone());
        if !has_more {
            if before != 0 {
                return Err(AgentSessionError::Failed);
            }
            break;
        }
        page=runtime.api.call("session/page",json!({"request":{"address":address,"throughSeq":cut,"beforeSeq":before,"maxMessages":50}})).await?;
    }
    Ok(pages.into_iter().rev().flatten().collect())
}
