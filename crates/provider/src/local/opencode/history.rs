//! ACP replay is the sole authoritative transcript; list/load never submit model input.
mod previews;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};
use serde_json::{Value, json};

pub(super) use previews::populate;

use super::{OpenCodeClient, PROVIDER, config, launcher, streaming::Stream};
use crate::{
    local::acp_transport::Transport,
    ports::{
        agent_session::AgentSessionError,
        native_history::{ListOptions, SessionDescriptor, SessionHistory},
    },
    protocol::timeline::NativeItem,
};

/// Incrementally load this native transcript; fail on RPC, identity or projection errors.
pub(super) async fn replay(
    transport: &mut Transport,
    id: &str,
    cwd: &str,
    images: crate::local::images::ImageStore,
) -> Result<(Value, Stream), AgentSessionError> {
    let mut stream = Stream::new(images);
    let mut completed = VecDeque::new();
    let result = transport
        .request_with_updates(
            "session/load",
            json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
            |message| {
                if message["method"] == "session/update" {
                    if message["params"]["sessionId"] != id {
                        return Err(AgentSessionError::Failed);
                    }
                    stream.update(&message["params"]["update"])?;
                    // Progress does not belong in a complete history replay.
                    completed.extend(stream.events.drain(..).filter(|event| {
                        matches!(
                            event,
                            crate::ports::agent_session::AgentTurnEvent::Timeline(_)
                        )
                    }));
                }
                Ok(())
            },
        )
        .await?;
    stream.flush();
    completed.extend(stream.events.drain(..).filter(|event| {
        matches!(
            event,
            crate::ports::agent_session::AgentTurnEvent::Timeline(_)
        )
    }));
    stream.events = completed;
    Ok((config::state(&result)?, stream))
}

/// Drain immutable replay entries and restore host input correlation from saved native IDs.
pub(super) fn entries(stream: &mut Stream, clients: &BTreeMap<String, String>) -> Vec<NativeItem> {
    stream
        .events
        .drain(..)
        .filter_map(|event| {
            if let crate::ports::agent_session::AgentTurnEvent::Timeline(mut entry) = event {
                if entry.item["type"] == "user_message"
                    && let Some(client) = entry.item["messageId"]
                        .as_str()
                        .and_then(|id| clients.get(id))
                {
                    entry.item["clientMessageId"] = json!(client);
                }
                Some(entry)
            } else {
                None
            }
        })
        .collect()
}

/// Read bounded native pages for the exact cwd; reject invalid limits or malformed pages.
pub(super) async fn list(
    transport: &mut Transport,
    options: &ListOptions,
) -> Result<Vec<SessionDescriptor>, AgentSessionError> {
    if !(1..=4096).contains(&options.scan_limit) {
        return Err(AgentSessionError::Rejected);
    }
    let mut results = Vec::new();
    let mut cursors = BTreeSet::new();
    let mut identities = BTreeSet::new();
    let mut cursor = Value::Null;
    loop {
        let mut params = json!({});
        if let Some(cwd) = &options.cwd {
            params["cwd"] = json!(cwd);
        }
        if !cursor.is_null() {
            params["cursor"] = cursor;
        }
        let response = transport.request("session/list", params).await?;
        let rows = response["sessions"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?;
        for row in rows.iter().take(options.scan_limit - results.len()) {
            let id = config::text(row, "sessionId")?;
            if !identities.insert(id.to_owned()) {
                return Err(AgentSessionError::Failed);
            }
            let cwd = row["cwd"]
                .as_str()
                .filter(|cwd| std::path::Path::new(cwd).is_absolute())
                .ok_or(AgentSessionError::Failed)?;
            if options
                .cwd
                .as_deref()
                .is_some_and(|expected| expected != cwd)
            {
                return Err(AgentSessionError::Failed);
            }
            let updated = config::text(row, "updatedAt")?;
            chrono::DateTime::parse_from_rfc3339(updated).map_err(|_| AgentSessionError::Failed)?;
            results.push(SessionDescriptor {
                provider_id: PROVIDER.into(),
                provider_label: "OpenCode".into(),
                provider_handle_id: id.into(),
                cwd: cwd.into(),
                title: row["title"].as_str().map(str::to_owned),
                first_prompt_preview: None,
                last_prompt_preview: None,
                last_activity_at: updated.into(),
            });
        }
        if results.len() >= options.scan_limit {
            break;
        }
        cursor = response["nextCursor"].clone();
        if cursor.is_null() {
            break;
        }
        let next = cursor
            .as_str()
            .filter(|cursor| !cursor.is_empty() && cursor.len() <= 1024)
            .ok_or(AgentSessionError::Failed)?;
        if !cursors.insert(next.to_owned()) {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(results)
}

/// Read native session metadata and full history without prompting or applying overrides.
/// Invalid handles are rejected; child, protocol and replay failures are propagated.
pub(super) async fn inspect(
    client: &OpenCodeClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<SessionHistory, AgentSessionError> {
    super::session::validate_handle(handle)?;
    let spec = crate::ports::agent_session::AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: cwd.into(),
        config: StoredAgentConfig::default(),
    };
    config::validate_spec(&spec)?;
    let (mut transport, _) = launcher::spawn(client, cwd, &spec.config).await?;
    let result = async {
        let descriptors = list(
            &mut transport,
            &ListOptions {
                cwd: Some(cwd.into()),
                scan_limit: 4096,
            },
        )
        .await?;
        let mut descriptor = descriptors
            .into_iter()
            .find(|entry| entry.provider_handle_id == handle.session_id)
            .ok_or(AgentSessionError::Rejected)?;
        let (options, mut stream) = replay(
            &mut transport,
            &handle.session_id,
            cwd,
            client.images.clone(),
        )
        .await?;
        let entries = entries(&mut stream, &super::session::clients(handle)?);
        (
            descriptor.first_prompt_preview,
            descriptor.last_prompt_preview,
        ) = previews::from_entries(&entries);
        let config = config::stored(&options);
        Ok(SessionHistory {
            resume_metadata: BTreeMap::from([(
                "opencode".into(),
                json!({"config":config,"model":config.model,"clients":{}}),
            )]),
            parent_id: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            descriptor,
            config,
            active: false,
            entries,
        })
    }
    .await;
    let closed = transport.close().await;
    closed?;
    result
}
