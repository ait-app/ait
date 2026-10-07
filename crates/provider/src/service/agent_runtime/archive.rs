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
pub(crate) fn archive_workspaces(
    registry: &dyn AgentRuntimeRegistry,
    workspaces: &[String],
    timestamp: &str,
) -> Result<Vec<String>, AgentRuntimeError> {
    let workspaces: BTreeSet<_> = workspaces.iter().map(String::as_str).collect();
    let records = registry.list().map_err(map_agent_registry)?;
    let selected: BTreeSet<_> = records
        .iter()
        .filter(|record| {
            record
                .workspace_id
                .as_deref()
                .is_some_and(|id| workspaces.contains(id))
        })
        .map(|record| record.id.clone())
        .collect();
    for record in &records {
        if selected.contains(&record.id) {
            archive_one(registry, &record.id, timestamp)?;
        } else if record.archived_at.is_none()
            && record
                .labels
                .get(PARENT_AGENT_ID_LABEL)
                .is_some_and(|parent| selected.contains(parent))
        {
            detach(registry, &record.id, timestamp)?;
        }
    }
    Ok(selected.into_iter().collect())
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
