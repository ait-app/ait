//! Agent-owned implementation of the Workspace attention boundary.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use domain::agent_runtime::registry::{AgentRuntimeRegistry, AgentRuntimeRegistryError};
use domain::agent_runtime::{
    AgentAttentionReason, AgentRuntimeStatus, PersistedAgentRuntimeRecord,
};
use domain::workspace::activity::WorkspaceStateBucket;
use domain::workspace::attention::{
    WorkspaceActivity, WorkspaceAttentionChanges, WorkspaceStateError,
};
use model::workspace::attention::{
    WorkspaceActivitySource, WorkspaceAttention, WorkspaceAttentionScan,
};

const PARENT_AGENT_ID_LABEL: &str = "paseo.parent-agent-id";

/// Workspace attention adapter sharing the Agent directory's durable registry.
#[derive(Debug)]
pub struct AgentWorkspaceAttention {
    agents: Box<dyn AgentRuntimeRegistry>,
    timeline: Option<crate::storage::timeline::Timeline>,
}

impl AgentWorkspaceAttention {
    /// Wrap the shared Agent registry without introducing another cache or writer.
    #[must_use]
    pub fn new(agents: Box<dyn AgentRuntimeRegistry>) -> Self {
        Self {
            agents,
            timeline: None,
        }
    }

    /// Include running native children from the same timeline used by Agent execution.
    #[must_use]
    pub fn with_timeline(mut self, timeline: crate::storage::timeline::Timeline) -> Self {
        self.timeline = Some(timeline);
        self
    }
}

impl WorkspaceActivitySource for AgentWorkspaceAttention {
    fn snapshot(&self) -> Result<Vec<WorkspaceActivity>, WorkspaceStateError> {
        let agents = self.agents.list().map_err(map_agent_error)?;
        let by_id = agents
            .iter()
            .filter(|agent| !agent.internal && !is_archived(agent.archived_at.as_deref()))
            .map(|agent| (agent.id.as_str(), agent))
            .collect::<BTreeMap<_, _>>();
        let mut activity: Vec<_> = by_id
            .values()
            .filter_map(|agent| {
                let root_id = workspace_root_id(agent, &by_id)?;
                let root = by_id.get(root_id)?;
                let is_root = root_id == agent.id;
                if !is_root && agent.last_status != AgentRuntimeStatus::Running {
                    return None;
                }
                let bucket = if is_root {
                    agent_bucket(agent)
                } else {
                    WorkspaceStateBucket::Running
                };
                Some(WorkspaceActivity {
                    workspace_id: root.workspace_id.clone()?,
                    bucket,
                    changed_at: Some(
                        agent
                            .attention_timestamp
                            .as_deref()
                            .unwrap_or_else(|| effective_timestamp(agent))
                            .to_owned(),
                    ),
                })
            })
            .collect();
        if let Some(timeline) = &self.timeline {
            for (parent, changed_at) in timeline
                .running_subagent_activity()
                .map_err(|_| WorkspaceStateError::AgentRegistry)?
            {
                let Some(agent) = by_id.get(parent.as_str()) else {
                    continue;
                };
                let Some(root_id) = workspace_root_id(agent, &by_id) else {
                    continue;
                };
                let Some(workspace_id) = by_id
                    .get(root_id)
                    .and_then(|root| root.workspace_id.as_ref())
                else {
                    continue;
                };
                activity.push(WorkspaceActivity {
                    workspace_id: workspace_id.clone(),
                    bucket: WorkspaceStateBucket::Running,
                    changed_at,
                });
            }
        }
        Ok(activity)
    }
}

fn agent_bucket(agent: &PersistedAgentRuntimeRecord) -> WorkspaceStateBucket {
    if agent.attention_reason == Some(AgentAttentionReason::Permission) {
        WorkspaceStateBucket::NeedsInput
    } else if agent.last_status == AgentRuntimeStatus::Error
        || agent.attention_reason == Some(AgentAttentionReason::Error)
    {
        WorkspaceStateBucket::Failed
    } else if agent.last_status == AgentRuntimeStatus::Running {
        WorkspaceStateBucket::Running
    } else if agent.requires_attention {
        WorkspaceStateBucket::Attention
    } else {
        WorkspaceStateBucket::Done
    }
}

impl WorkspaceAttention for AgentWorkspaceAttention {
    fn scan(&self) -> Result<Box<dyn WorkspaceAttentionScan + '_>, WorkspaceStateError> {
        let records = self.agents.list().map_err(map_agent_error)?;
        Ok(Box::new(AttentionScan {
            records,
            registry: self.agents.as_ref(),
        }))
    }

    /// Mark the newest finished, read root Agent in an active Workspace as unread.
    ///
    /// # Errors
    /// Returns an inline business error for missing Workspace/candidate and a categorized registry
    /// failure when durable state cannot be read or changed.
    fn mark_unread(
        &self,
        workspace_id: &str,
        updated_at: &str,
    ) -> Result<String, WorkspaceStateError> {
        let agents = self.agents.list().map_err(map_agent_error)?;
        let by_id = agents
            .iter()
            .filter(|agent| !agent.internal)
            .map(|agent| (agent.id.as_str(), agent))
            .collect::<BTreeMap<_, _>>();
        let mut candidates = agents
            .iter()
            .filter(|agent| !agent.internal && !is_archived(agent.archived_at.as_deref()))
            .filter(|agent| agent.workspace_id.as_deref() == Some(workspace_id))
            .filter(|agent| workspace_root_id(agent, &by_id) == Some(agent.id.as_str()))
            .filter(|agent| {
                matches!(
                    agent.last_status,
                    AgentRuntimeStatus::Idle | AgentRuntimeStatus::Closed
                )
            })
            .filter(|agent| !agent.requires_attention)
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            compare_agent_updated_at(right, left).then_with(|| left.id.cmp(&right.id))
        });
        let candidate = candidates
            .first()
            .copied()
            .ok_or_else(|| WorkspaceStateError::NoFinishedAgent(workspace_id.to_owned()))?;
        let agent_id = candidate.id.clone();
        let mut desired = candidate.clone();
        desired.updated_at = monotonic_timestamp(&candidate.updated_at, updated_at);
        desired.requires_attention = true;
        desired.attention_reason = Some(AgentAttentionReason::Finished);
        desired.attention_timestamp = Some(desired.updated_at.clone());
        let updated = self
            .agents
            .update(&agent_id, &|current| {
                if current == candidate {
                    desired.clone()
                } else {
                    current.clone()
                }
            })
            .map_err(map_agent_error)?
            .ok_or_else(|| WorkspaceStateError::AgentNoLongerFinished(agent_id.clone()))?;
        if updated != desired {
            return Err(WorkspaceStateError::AgentNoLongerFinished(agent_id));
        }
        Ok(updated.id)
    }
}

#[derive(Debug)]
struct AttentionScan<'a> {
    records: Vec<PersistedAgentRuntimeRecord>,
    registry: &'a dyn AgentRuntimeRegistry,
}

impl WorkspaceAttentionScan for AttentionScan<'_> {
    fn clear_attention(&self, workspace_id: &str, updated_at: &str) -> WorkspaceAttentionChanges {
        let mut result = WorkspaceAttentionChanges {
            cleared_agent_ids: Vec::new(),
            error: None,
        };
        for agent in self.records.iter().filter(|agent| {
            !agent.internal
                && !is_archived(agent.archived_at.as_deref())
                && agent.workspace_id.as_deref() == Some(workspace_id)
                && agent.requires_attention
                && agent.attention_reason != Some(AgentAttentionReason::Permission)
        }) {
            let next_updated_at = monotonic_timestamp(&agent.updated_at, updated_at);
            match self.registry.update(&agent.id, &|current| {
                // The batch snapshot can precede a new permission, completion or placement.
                // Compare under the registry's update lock before consuming its attention.
                if current != agent {
                    return current.clone();
                }
                let mut next = current.clone();
                next.updated_at.clone_from(&next_updated_at);
                next.requires_attention = false;
                next.attention_reason = None;
                next.attention_timestamp = None;
                next
            }) {
                Ok(Some(updated)) if !updated.requires_attention => {
                    result.cleared_agent_ids.push(agent.id.clone());
                }
                Ok(Some(_)) => {}
                Ok(None) => {
                    result.error =
                        Some(WorkspaceStateError::AgentNoLongerFinished(agent.id.clone()));
                    return result;
                }
                Err(error) => {
                    result.error = Some(map_agent_error(error));
                    return result;
                }
            }
        }
        result
    }
}

fn workspace_root_id<'a>(
    agent: &'a PersistedAgentRuntimeRecord,
    agents: &BTreeMap<&str, &'a PersistedAgentRuntimeRecord>,
) -> Option<&'a str> {
    let mut seen = std::collections::BTreeSet::from([agent.id.as_str()]);
    let mut current = agent;
    loop {
        let Some(parent_id) = current
            .labels
            .get(PARENT_AGENT_ID_LABEL)
            .map(String::as_str)
            .filter(|parent_id| !parent_id.is_empty())
        else {
            return Some(current.id.as_str());
        };
        if !seen.insert(parent_id) {
            return None;
        }
        let parent = agents.get(parent_id)?;
        if parent.workspace_id != current.workspace_id {
            return Some(current.id.as_str());
        }
        current = parent;
    }
}

fn compare_agent_updated_at(
    left: &PersistedAgentRuntimeRecord,
    right: &PersistedAgentRuntimeRecord,
) -> Ordering {
    let left = effective_timestamp(left);
    let right = effective_timestamp(right);
    match (parse_timestamp(left), parse_timestamp(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn effective_timestamp(record: &PersistedAgentRuntimeRecord) -> &str {
    let updated = parse_timestamp(&record.updated_at);
    let activity = record.last_activity_at.as_deref().and_then(parse_timestamp);
    if activity.is_some_and(|activity| updated.is_none_or(|updated| activity > updated)) {
        record
            .last_activity_at
            .as_deref()
            .unwrap_or(&record.updated_at)
    } else {
        &record.updated_at
    }
}

fn monotonic_timestamp(previous: &str, proposed: &str) -> String {
    let previous = parse_timestamp(previous);
    let parsed_proposed = parse_timestamp(proposed);
    match (previous, parsed_proposed) {
        (Some(previous), Some(parsed_proposed)) if parsed_proposed <= previous => (previous
            + Duration::milliseconds(1))
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true),
        (_, Some(parsed_proposed)) => parsed_proposed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
        _ => proposed.to_owned(),
    }
}

fn parse_timestamp(value: &str) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn is_archived(archived_at: Option<&str>) -> bool {
    archived_at.is_some_and(|value| !value.is_empty())
}

const fn map_agent_error(_: AgentRuntimeRegistryError) -> WorkspaceStateError {
    WorkspaceStateError::AgentRegistry
}

#[cfg(test)]
mod tests;
