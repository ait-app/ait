//! Read a fixed native journal cut without creating a session or sending a prompt.
use super::super::{DeepSeekHarnessClient, PROVIDER, streaming::Stream};
use super::{content, projection, runtime::Runtime};
use crate::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};
use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A fixed journal cut and its native metadata, shared by replay and import.
pub(super) struct Journal {
    pub(super) header: Value,
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
    runtime
        .subscribe(
            "history",
            "session/follow",
            json!({"request":{"address":address,"maxMessages":50}}),
        )
        .await?;
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let frame = runtime.next().await?;
            if frame["type"] == "item"
                && frame["streamId"] == "events"
                && frame["value"]["type"] == "emit"
            {
                continue;
            }
            if frame["type"] != "item"
                || frame["streamId"] != "history"
                || frame["value"]["type"] != "snapshot"
            {
                return Err(AgentSessionError::Failed);
            }
            return Ok(frame["value"].clone());
        }
    })
    .await
    .map_err(|_| AgentSessionError::Failed)??;
    if snapshot["header"]["id"] != session_id
        || snapshot["header"]["cwd"] != cwd
        || snapshot["header"]["origin"] == "subagent"
    {
        return Err(AgentSessionError::Failed);
    }
    let cut = snapshot["cursor"]
        .as_i64()
        .ok_or(AgentSessionError::Failed)?;
    let header = snapshot["header"].clone();
    let values = snapshot["projections"]["values"].clone();
    let records = pages(runtime, &address, cut, snapshot).await?;
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
    Ok(Journal {
        header,
        values,
        entries,
        active,
    })
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
