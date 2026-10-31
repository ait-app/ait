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

/// Load this native transcript within `timeout` and return the directory it was loaded from.
///
/// The native `session_move` tool relocates a session to another directory of its project,
/// after which loading by the previous cwd is rejected; follow the session to where it lives.
pub(super) async fn replay(
    transport: &mut Transport,
    id: &str,
    cwd: &str,
    images: crate::local::images::ImageStore,
    timeout: std::time::Duration,
) -> Result<(Value, Stream, String), AgentSessionError> {
    let previous = transport.diagnostics.failure();
    match load(transport, id, cwd, images.clone(), timeout).await {
        Err(AgentSessionError::Rejected) => {
            let Some(location) = locate(transport, id)
                .await?
                .filter(|location| location != cwd)
            else {
                return Err(AgentSessionError::Rejected);
            };
            let (options, stream) = load(transport, id, &location, images, timeout).await?;
            transport.diagnostics.restore(previous);
            Ok((options, stream, location))
        }
        result => result.map(|(options, stream)| (options, stream, cwd.to_owned())),
    }
}

/// Find the directory the native CLI currently files this session under, if it still exists.
async fn locate(transport: &mut Transport, id: &str) -> Result<Option<String>, AgentSessionError> {
    let sessions = list(
        transport,
        &ListOptions {
            cwd: None,
            scan_limit: 4096,
        },
    )
    .await?;
    Ok(sessions
        .into_iter()
        .find(|session| session.provider_handle_id == id)
        .map(|session| session.cwd))
}

/// Incrementally load this native transcript within `timeout`, restoring ordinary controls after.
/// Cold restoration uses the startup budget; settlement retains the ordinary control budget.
/// Fail on RPC, identity or projection errors.
async fn load(
    transport: &mut Transport,
    id: &str,
    cwd: &str,
    images: crate::local::images::ImageStore,
    timeout: std::time::Duration,
) -> Result<(Value, Stream), AgentSessionError> {
    let mut stream = Stream::new(images);
    let mut completed = VecDeque::new();
    let mut child_bytes = 0usize;
    let ordinary_timeout = transport.control_deadline();
    transport.set_control_deadline(timeout);
    let result = transport
        .request_with_updates(
            "session/load",
            json!({"sessionId":id,"cwd":cwd,"mcpServers":[]}),
            |message| {
                if message["method"] == "opencode/session/child_update"
                    || (message["method"] == "session/update"
                        && message["params"]["update"]
                            .pointer("/_meta/opencode~1child-session")
                            .is_some())
                {
                    child_bytes = child_bytes.saturating_add(
                        serde_json::to_vec(message)
                            .map_err(|_| AgentSessionError::Failed)?
                            .len(),
                    );
                    if stream.child_frames.len() >= 256 || child_bytes > 512 * 1024 {
                        return Err(AgentSessionError::Failed);
                    }
                    stream.child_frames.push(message.clone());
                    return Ok(());
                }
                if message["method"] == "session/update" {
                    if message["params"]["sessionId"] != id {
                        // One ACP process can load multiple native sessions. A preceding load
                        // may publish its commands after its response; never mix those updates
                        // into the selected transcript or reject the next child's replay.
                        return Ok(());
                    }
                    stream.links.update(&message["params"]["update"])?;
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
        .await;
    transport.set_control_deadline(ordinary_timeout);
    let result = result?;
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

/// Collect native inputs and standalone compactions before replay entries are drained.
pub(super) fn identities(stream: &Stream) -> BTreeSet<String> {
    stream
        .users
        .iter()
        .cloned()
        .chain(stream.events.iter().filter_map(|event| {
            let crate::ports::agent_session::AgentTurnEvent::Timeline(entry) = event else {
                return None;
            };
            entry.item["compactionId"].as_str().map(str::to_owned)
        }))
        .collect()
}

/// Restore accepted input identity for each user group and standalone native compaction.
pub(super) fn entries(stream: &mut Stream, clients: &BTreeMap<String, String>) -> Vec<NativeItem> {
    stream
        .events
        .drain(..)
        .filter_map(|event| {
            if let crate::ports::agent_session::AgentTurnEvent::Timeline(mut entry) = event {
                if let Some(turn) = entry.item["compactionId"]
                    .as_str()
                    .and_then(|id| clients.get(id))
                    .or_else(|| entry.turn_id.as_ref().and_then(|id| clients.get(id)))
                {
                    entry.turn_id = Some(turn.clone());
                }
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
    let (mut transport, _, source) = launcher::observed(client, cwd, &spec.config).await?;
    let result = async {
        let child = super::subagents::history::verify(&mut transport, client, handle, cwd).await?;
        let (options, mut stream, location) = replay(
            &mut transport,
            &handle.session_id,
            cwd,
            client.images.clone(),
            client.startup_timeout,
        )
        .await?;
        let descriptors = list(
            &mut transport,
            &ListOptions {
                cwd: Some(location.clone()),
                scan_limit: 4096,
            },
        )
        .await?;
        let mut descriptor = descriptors
            .into_iter()
            .find(|entry| entry.provider_handle_id == handle.session_id)
            .or_else(|| {
                child.as_ref().map(|child| SessionDescriptor {
                    provider_id: PROVIDER.into(),
                    provider_label: "OpenCode".into(),
                    provider_handle_id: child.id.clone(),
                    cwd: location.clone(),
                    title: child.descriptor["title"].as_str().map(str::to_owned),
                    first_prompt_preview: None,
                    last_prompt_preview: None,
                    last_activity_at: child.descriptor["updatedAt"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .ok_or(AgentSessionError::Rejected)?;
        let clients = super::session::clients(handle)?;
        let mut entries = entries(&mut stream, &clients);
        let mut usage = None;
        if let Some(source) = source {
            if let Ok(observation) = source.bind(&handle.session_id, &location).await {
                observation.history(&mut entries, &clients);
                usage = observation.usage();
            } else {
                tracing::warn!("OpenCode native history metadata unavailable");
            }
        }
        (
            descriptor.first_prompt_preview,
            descriptor.last_prompt_preview,
        ) = previews::from_entries(&entries);
        let config = config::stored(&options);
        let mut resume_metadata = BTreeMap::from([(
            "opencode".into(),
            json!({"config":config,"model":config.model,"clients":clients}),
        )]);
        if let Some(usage) = usage {
            resume_metadata.insert("lastUsage".into(), json!(usage));
        }
        Ok(SessionHistory {
            resume_metadata,
            parent_id: child.map(|child| child.parent_id),
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
