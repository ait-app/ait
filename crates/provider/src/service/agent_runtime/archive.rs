//! Shared explicit and automatic archive behavior over the Agent registry.

use std::collections::{BTreeMap, BTreeSet};

use domain::agent_runtime::registry::AgentRuntimeRegistry;

use super::{
    AgentRuntimeError, AgentRuntimeStatus, OPEN_AGENT_TAB_LABEL_PREFIX, PARENT_AGENT_ID_LABEL,
    PersistedAgentRuntimeRecord, map_agent_registry,
};

pub(crate) fn archive(
    registry: &dyn AgentRuntimeRegistry,
    id: &str,
    timestamp: &str,
) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
    let root = registry
        .get(id)
        .map_err(map_agent_registry)?
        .ok_or_else(|| AgentRuntimeError::NotFound(id.to_owned()))?;
    if root.archived_at.is_some() {
        return Ok(root);
    }
    if root
        .observation
        .as_ref()
        .is_some_and(|source| !source.status.is_terminal())
    {
        return Err(AgentRuntimeError::InvalidRequest);
    }
    let archived = archive_one(registry, id, timestamp)?;
    let mut children = BTreeMap::<String, Vec<PersistedAgentRuntimeRecord>>::new();
    for record in registry.list().map_err(map_agent_registry)? {
        if !record.internal
            && record.archived_at.is_none()
            && let Some(parent) = record.labels.get(PARENT_AGENT_ID_LABEL)
        {
            children.entry(parent.clone()).or_default().push(record);
        }
    }
    let mut pending = vec![archived.clone()];
    while let Some(parent) = pending.pop() {
        for child in children.remove(&parent.id).unwrap_or_default() {
            if child.observation.is_some() {
                pending.push(archive_one(registry, &child.id, timestamp)?);
                continue;
            }
            let open = child.labels.iter().any(|(label, value)| {
                label.starts_with(OPEN_AGENT_TAB_LABEL_PREFIX) && value == "true"
            });
            let cross_workspace = parent.workspace_id.is_some()
                && child.workspace_id.is_some()
                && parent.workspace_id != child.workspace_id;
            if open || cross_workspace {
                detach(registry, &child.id, timestamp)?;
            } else {
                pending.push(archive_one(registry, &child.id, timestamp)?);
            }
        }
    }
    Ok(archived)
}

/// Archive every Agent owned by a Workspace, retaining children placed in other Workspaces.
/// Observed children cannot be detached from their verified parent, so they are archived with
/// it, as in [`archive`], and are included in the returned IDs.
pub(crate) fn archive_workspaces(
    registry: &dyn AgentRuntimeRegistry,
    workspaces: &[String],
    timestamp: &str,
) -> Result<Vec<String>, AgentRuntimeError> {
    let workspaces: BTreeSet<_> = workspaces.iter().map(String::as_str).collect();
    let records = registry.list().map_err(map_agent_registry)?;
    let mut archived = BTreeSet::new();
    let mut children = BTreeMap::<&str, Vec<&PersistedAgentRuntimeRecord>>::new();
    for record in &records {
        if record
            .workspace_id
            .as_deref()
            .is_some_and(|id| workspaces.contains(id))
        {
            archived.insert(record.id.clone());
        } else if record.archived_at.is_none()
            && let Some(parent) = record.labels.get(PARENT_AGENT_ID_LABEL)
        {
            children.entry(parent).or_default().push(record);
        }
    }
    for id in &archived {
        archive_one(registry, id, timestamp)?;
    }
    let mut pending: Vec<_> = archived.iter().cloned().collect();
    while let Some(parent) = pending.pop() {
        for child in children.remove(parent.as_str()).unwrap_or_default() {
            if child.observation.is_some() {
                archive_one(registry, &child.id, timestamp)?;
                archived.insert(child.id.clone());
                pending.push(child.id.clone());
            } else {
                detach(registry, &child.id, timestamp)?;
            }
        }
    }
    Ok(archived.into_iter().collect())
}

fn archive_one(
    registry: &dyn AgentRuntimeRegistry,
    id: &str,
    timestamp: &str,
) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
    registry
        .update(id, &|current| {
            if current.archived_at.is_some() {
                return current.clone();
            }
            let mut next = current.clone();
            next.archived_at = Some(timestamp.to_owned());
            if let Some(source) = &mut next.observation {
                if !source.status.is_terminal() {
                    if source.health
                        != domain::agent_runtime::observation::ObservationHealth::Unavailable
                    {
                        source.revision = source.revision.saturating_add(1);
                    }
                    source.health =
                        domain::agent_runtime::observation::ObservationHealth::Unavailable;
                }
                next.last_status = AgentRuntimeStatus::Closed;
            }
            if matches!(
                next.last_status,
                AgentRuntimeStatus::Running | AgentRuntimeStatus::Initializing
            ) {
                next.last_status = AgentRuntimeStatus::Idle;
            }
            next.requires_attention = false;
            next.attention_reason = None;
            next.attention_timestamp = None;
            next
        })
        .map_err(map_agent_registry)?
        .ok_or_else(|| AgentRuntimeError::NotFound(id.to_owned()))
}

pub(crate) fn detach(
    registry: &dyn AgentRuntimeRegistry,
    id: &str,
    timestamp: &str,
) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
    let current = registry
        .get(id)
        .map_err(map_agent_registry)?
        .ok_or_else(|| AgentRuntimeError::NotFound(id.to_owned()))?;
    if current.observation.is_some() {
        return Err(AgentRuntimeError::InvalidRequest);
    }
    if current
        .labels
        .get(PARENT_AGENT_ID_LABEL)
        .is_none_or(|parent| parent.trim().is_empty())
    {
        return Ok(current);
    }
    registry
        .update(id, &|current| {
            let mut next = current.clone();
            next.labels.remove(PARENT_AGENT_ID_LABEL);
            next.labels
                .retain(|label, _| !label.starts_with(OPEN_AGENT_TAB_LABEL_PREFIX));
            timestamp.clone_into(&mut next.updated_at);
            next
        })
        .map_err(map_agent_registry)?
        .ok_or_else(|| AgentRuntimeError::NotFound(id.to_owned()))
}
