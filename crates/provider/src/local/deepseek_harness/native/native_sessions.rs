//! Read-only discovery and import through the native Host's public session API.
use std::{collections::BTreeMap, path::Path};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

use super::{history, runtime::Runtime};
use crate::{
    local::deepseek_harness::{DeepSeekHarnessClient, PROVIDER, config::text},
    ports::{
        agent_session::AgentSessionError,
        native_history::{ListOptions, SessionDescriptor, SessionHistory},
    },
};

/// List visible root sessions, filtering directories before the bounded result limit.
/// Returns unsupported for ACP, or native transport/schema errors without creating an Agent.
pub(in crate::local::deepseek_harness) async fn list(
    client: &DeepSeekHarnessClient,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    if !client.interactive {
        return Err(AgentSessionError::Unavailable);
    }
    if !(1..=4096).contains(&options.scan_limit) {
        return Err(AgentSessionError::Rejected);
    }
    let cwd = options.cwd.clone().map_or_else(
        || std::env::current_dir().map_err(|_| AgentSessionError::Failed),
        |cwd| Ok(cwd.into()),
    )?;
    let mut runtime =
        Runtime::open(client, cwd.to_str().ok_or(AgentSessionError::Rejected)?).await?;
    let result = runtime
        .api
        .call("session/list", json!({"_request":{}}))
        .await;
    let closed = runtime.close().await;
    let response = result?;
    closed?;
    descriptors(&response, options)
}

fn descriptors(
    response: &Value,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    let mut entries = Vec::new();
    for row in response["items"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?
    {
        if row["blank"] == true || row["origin"] == "subagent" || row["cwd"].is_null() {
            continue;
        }
        let cwd = text(row, "cwd")?;
        if !Path::new(cwd).is_absolute() {
            return Err(AgentSessionError::Failed);
        }
        if options.cwd.as_ref().is_some_and(|filter| {
            cwd != filter
                && Path::new(cwd).canonicalize().ok().as_deref() != Some(Path::new(filter))
        }) {
            continue;
        }
        let id = text(row, "sessionId")?;
        validate_id(id)?;
        let updated = row["updatedAt"].as_i64().ok_or(AgentSessionError::Failed)?;
        entries.push((
            updated,
            SessionDescriptor {
                provider_id: PROVIDER.into(),
                provider_label: "DeepSeek Harness".into(),
                provider_handle_id: id.into(),
                cwd: cwd.into(),
                title: row["projections"]["values"]["title"]
                    .as_str()
                    .map(str::to_owned),
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: timestamp(updated)?,
            },
        ));
    }
    entries.sort_unstable_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.provider_handle_id.cmp(&right.1.provider_handle_id))
    });
    Ok(entries
        .into_iter()
        .take(options.scan_limit)
        .map(|(_, entry)| entry)
        .collect())
}

/// Inspect an external identity without creating/resuming a writer or altering configuration.
/// Returns validated history and non-secret resume metadata, or identity/history errors.
pub(in crate::local::deepseek_harness) async fn inspect(
    client: &DeepSeekHarnessClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    if !client.interactive {
        return Err(AgentSessionError::Unavailable);
    }
    validate_id(&handle.session_id)?;
    if handle.provider != PROVIDER
        || !Path::new(cwd).is_absolute()
        || handle
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("cwd"))
            .is_some_and(|saved| saved.as_str() != Some(cwd))
    {
        return Err(AgentSessionError::Rejected);
    }
    let mut runtime = Runtime::open(client, cwd).await?;
    let result = inspect_owned(&mut runtime, client, &handle.session_id, cwd).await;
    let closed = runtime.close().await;
    let history = result?;
    closed?;
    Ok(history)
}

async fn inspect_owned(
    runtime: &mut Runtime,
    client: &DeepSeekHarnessClient,
    id: &str,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    let journal = history::read_snapshot(runtime, client, id, cwd).await?;
    let model = match journal
        .values
        .pointer("/modelSelection/next")
        .filter(|value| value.is_object())
    {
        Some(model) => model.clone(),
        None => runtime.api.call("session/modelCatalog", json!({})).await?["default"].clone(),
    };
    let config = StoredAgentConfig {
        model: Some(json!([text(&model, "provider")?, text(&model, "model")?]).to_string()),
        thinking_option_id: model["reasoningEffort"].as_str().map(str::to_owned),
        mode_id: journal.values["permissions"]["currentValue"]
            .as_str()
            .filter(|mode| *mode != "custom")
            .map(str::to_owned),
        ..Default::default()
    };
    super::config::validate(&config)?;
    let created_at = timestamp(
        journal.header["createdAt"]
            .as_i64()
            .ok_or(AgentSessionError::Failed)?,
    )?;
    let prompts = journal
        .entries
        .iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .filter_map(|entry| entry.item["text"].as_str())
        .collect::<Vec<_>>();
    Ok(SessionHistory {
        resume_metadata: BTreeMap::from([
            ("cwd".into(), json!(cwd)),
            ("transport".into(), json!("native-host")),
        ]),
        parent_id: journal.header["parentSession"].as_str().map(str::to_owned),
        descriptor: SessionDescriptor {
            provider_id: PROVIDER.into(),
            provider_label: "DeepSeek Harness".into(),
            provider_handle_id: id.into(),
            cwd: cwd.into(),
            title: journal.values["title"].as_str().map(str::to_owned),
            first_prompt_preview: prompts.first().map(|text| (*text).to_owned()),
            last_prompt_preview: prompts.last().map(|text| (*text).to_owned()),
            last_activity_at: journal
                .entries
                .last()
                .map_or_else(|| created_at.clone(), |entry| entry.timestamp.clone()),
        },
        created_at,
        config,
        active: journal.active,
        entries: journal.entries,
    })
}

fn validate_id(id: &str) -> Result<(), AgentSessionError> {
    if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

fn timestamp(millis: i64) -> Result<String, AgentSessionError> {
    chrono::DateTime::from_timestamp_millis(millis)
        .map(|value| value.to_rfc3339())
        .ok_or(AgentSessionError::Failed)
}

#[cfg(all(test, unix))]
mod tests;
