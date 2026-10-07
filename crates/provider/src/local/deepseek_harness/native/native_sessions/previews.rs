//! Prefer native outline projections; bounded cold reads fill older missing caches.
use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::{Instant, timeout_at};

use super::{AgentSessionError, Runtime, SessionDescriptor};
use crate::local::session_preview;

type Previews = (Option<String>, Option<String>);

pub(super) fn projection(values: &Value) -> Previews {
    let mut prompts = values["turnOutline"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|turn| session_preview::text(turn["prompt"].as_str().into_iter()));
    let first = prompts.next();
    let last = prompts.next_back().or_else(|| first.clone());
    (first, last)
}

pub(super) async fn populate(runtime: &mut Runtime, sessions: &mut [SessionDescriptor]) {
    let deadline = Instant::now() + Duration::from_secs(10);
    for session in sessions {
        if Instant::now() >= deadline {
            break;
        }
        if session.first_prompt_preview.is_some() {
            continue;
        }
        let stream_id = format!("preview-{}", uuid::Uuid::new_v4());
        let result = timeout_at(
            deadline.min(Instant::now() + Duration::from_secs(2)),
            read(runtime, session, &stream_id),
        )
        .await;
        let _ = runtime.unsubscribe(&stream_id).await;
        if let Ok(Ok((first, last))) = result {
            session.first_prompt_preview = first;
            session.last_prompt_preview = last;
        }
    }
}

async fn read(
    runtime: &mut Runtime,
    session: &SessionDescriptor,
    stream_id: &str,
) -> Result<Previews, AgentSessionError> {
    let address = json!({"kind":"session","sessionId":session.provider_handle_id});
    runtime
        .subscribe(
            stream_id,
            "session/follow",
            json!({"request":{"address":address,"maxMessages":50}}),
        )
        .await?;
    let snapshot = loop {
        let frame = runtime.next().await?;
        if frame["streamId"] != stream_id {
            continue;
        }
        if frame["type"] != "item" || frame["value"]["type"] != "snapshot" {
            return Err(AgentSessionError::Failed);
        }
        break frame["value"].clone();
    };
    if snapshot["header"]["id"] != session.provider_handle_id
        || snapshot["header"]["cwd"] != session.cwd
    {
        return Err(AgentSessionError::Failed);
    }
    let projected = projection(&snapshot["projections"]["values"]);
    if projected.0.is_some() {
        return Ok(projected);
    }
    let cut = snapshot["cursor"]
        .as_i64()
        .ok_or(AgentSessionError::Failed)?;
    let mut before = cut.checked_add(1).ok_or(AgentSessionError::Failed)?;
    let mut page = snapshot;
    let mut bytes = 0;
    let mut first = None;
    let mut last = None;
    for _ in 0..100 {
        bytes += page.to_string().len();
        if bytes > 8 * 1024 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        let records = page["records"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?;
        for record in records.iter().rev() {
            let event = &record["event"];
            if event["type"] != "user/message"
                || !matches!(
                    event["data"]["source"]["kind"].as_str(),
                    None | Some("user")
                )
            {
                continue;
            }
            let prompt = session_preview::text(
                event["data"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|part| part["type"] == "text")
                    .filter_map(|part| part["text"].as_str()),
            );
            if let Some(prompt) = prompt {
                if last.is_none() {
                    last = Some(prompt.clone());
                }
                first = Some(prompt);
            }
        }
        if page["hasMore"] == false {
            return Ok((first, last));
        }
        let next = records
            .first()
            .and_then(|record| record["event"]["seq"].as_i64())
            .ok_or(AgentSessionError::Failed)?;
        if next <= 0 || next >= before {
            return Err(AgentSessionError::Failed);
        }
        before = next;
        page = runtime.api.call("session/page", json!({"request":{"address":address,"throughSeq":cut,"beforeSeq":before,"maxMessages":50}})).await?;
    }
    Err(AgentSessionError::Failed)
}
