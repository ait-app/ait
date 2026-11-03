//! Relationship discovery uses ACP task results, never prompt text or native database inspection.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig};

use super::{LOCATOR, MAX_CHILDREN};
use crate::{
    local::{
        acp_transport::Transport,
        opencode::{OpenCodeClient, config, history, launcher},
    },
    ports::{
        agent_session::AgentSessionError, controls::NativeSubagent, native_history::ListOptions,
    },
};

/// Discover durable native task relationships in this directory under a single bounded deadline.
pub(in crate::local::opencode) async fn discover(
    client: &OpenCodeClient,
    cwd: &str,
) -> Result<Vec<NativeSubagent>, AgentSessionError> {
    let (mut transport, capabilities) =
        launcher::spawn(client, cwd, &StoredAgentConfig::default()).await?;
    let result = tokio::time::timeout(client.startup_timeout, async {
        if capabilities["loadSession"] != true
            || !capabilities["sessionCapabilities"]["list"].is_object()
        {
            return Err(AgentSessionError::Unavailable);
        }
        let sessions = history::list(
            &mut transport,
            &ListOptions {
                cwd: Some(cwd.into()),
                scan_limit: 4096,
            },
        )
        .await?;
        let activity: BTreeMap<_, _> = sessions
            .iter()
            .map(|session| {
                (
                    session.provider_handle_id.clone(),
                    session.last_activity_at.clone(),
                )
            })
            .collect();
        let mut children = BTreeMap::new();
        let mut inspected = BTreeSet::new();
        for session in sessions {
            if !inspected.insert(session.provider_handle_id.clone()) {
                continue;
            }
            let (_, stream, location) = history::replay(
                &mut transport,
                &session.provider_handle_id,
                &session.cwd,
                client.images.clone(),
                client.deadline,
            )
            .await?;
            let links = stream
                .links
                .descriptors(&session.provider_handle_id, &location)?;
            for mut child in
                restore(&mut transport, client, &session.provider_handle_id, links).await?
            {
                inspected.insert(child.id.clone());
                // A historical task cannot supersede a newer completion observed by the host.
                let updated = activity.get(&child.id).unwrap_or(&session.last_activity_at);
                child.descriptor["createdAt"] = serde_json::json!(updated);
                child.descriptor["updatedAt"] = serde_json::json!(updated);
                if children.len() >= MAX_CHILDREN && !children.contains_key(&child.id) {
                    return Err(AgentSessionError::Failed);
                }
                if let Some(previous) = children.insert(child.id.clone(), child.clone())
                    && previous.parent_id != child.parent_id
                {
                    return Err(AgentSessionError::Failed);
                }
            }
        }
        Ok(children.into_values().collect())
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?;
    transport.close().await?;
    result
}

/// Walk native task links breadth-first; reject cycles, conflicting ownership and unbounded trees.
pub(in crate::local::opencode) async fn restore(
    transport: &mut Transport,
    client: &OpenCodeClient,
    root: &str,
    children: Vec<NativeSubagent>,
) -> Result<Vec<NativeSubagent>, AgentSessionError> {
    let mut pending = VecDeque::from(children);
    let mut seen = BTreeMap::from([(root.to_owned(), String::new())]);
    let mut result = Vec::new();
    while let Some(mut child) = pending.pop_front() {
        if let Some(parent) = seen.get(&child.id) {
            if parent != &child.parent_id {
                return Err(AgentSessionError::Failed);
            }
            continue;
        }
        if result.len() >= MAX_CHILDREN {
            return Err(AgentSessionError::Failed);
        }
        seen.insert(child.id.clone(), child.parent_id.clone());
        let (options, stream, location) = history::replay(
            transport,
            &child.id,
            &child.cwd,
            client.images.clone(),
            client.deadline,
        )
        .await?;
        let model = config::stored(&options).model;
        let agent = child.descriptor["subtitle"].as_str();
        let mut parts: Vec<_> = agent
            .into_iter()
            .flat_map(|agent| agent.split(" · "))
            .collect();
        if let Some(model) = model.as_deref()
            && !parts.contains(&model)
        {
            parts.push(model);
        }
        let subtitle = parts.join(" · ");
        child.descriptor["subtitle"] =
            serde_json::json!((!subtitle.is_empty()).then_some(subtitle));
        child.cwd.clone_from(&location);
        child.descriptor["cwd"] = serde_json::json!(location);
        pending.extend(stream.links.descriptors(&child.id, &location)?);
        result.push(child);
    }
    Ok(result)
}

/// Verify the locator against its parent's native task output before serving an independent child.
pub(in crate::local::opencode) async fn verify(
    transport: &mut Transport,
    client: &OpenCodeClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> Result<Option<NativeSubagent>, AgentSessionError> {
    let Some(locator) = handle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(LOCATOR))
    else {
        return Ok(None);
    };
    let parent = config::text(locator, "parent")?;
    if parent == handle.session_id || locator["cwd"] != cwd {
        return Err(AgentSessionError::Rejected);
    }
    let (_, stream, location) = history::replay(
        transport,
        parent,
        cwd,
        client.images.clone(),
        client.startup_timeout,
    )
    .await?;
    stream
        .links
        .descriptors(parent, &location)?
        .into_iter()
        .find(|child| child.id == handle.session_id)
        .map(Some)
        .ok_or(AgentSessionError::Rejected)
}
