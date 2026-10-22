//! Lightweight directory projections; provider configuration and native handles stay local.

use std::sync::Arc;

use anyhow::Context;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use model::changes::Changes;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(crate) struct Sources {
    pub(crate) projects: Arc<dyn ProjectRegistry>,
    pub(crate) workspaces: Arc<dyn WorkspaceRegistry>,
    pub(crate) agents: Arc<dyn AgentRuntimeRegistry>,
    pub(crate) changes: Changes,
}

impl Sources {
    pub(super) fn snapshots(&self) -> anyhow::Result<[(&'static str, Value); 3]> {
        let projects = self
            .projects
            .list()
            .context("read Hub project projection")?
            .into_iter()
            .filter(|record| record.archived_at.is_none())
            .map(|record| {
                json!({
                    "id": record.project_id,
                    "data": {
                        "name": record.display_name(),
                        "kind": record.kind,
                        "createdAt": record.created_at,
                        "updatedAt": record.updated_at
                    }
                })
            })
            .collect::<Vec<_>>();
        let workspaces = self
            .workspaces
            .list()
            .context("read Hub workspace projection")?
            .into_iter()
            .filter(|record| record.archived_at.is_none())
            .map(|record| {
                json!({
                    "id": record.workspace_id,
                    "data": {
                        "title": record.display_name(),
                        "projectId": record.project_id,
                        "kind": record.kind,
                        "branch": record.branch,
                        "createdAt": record.created_at,
                        "updatedAt": record.updated_at
                    }
                })
            })
            .collect::<Vec<_>>();
        let agents = self
            .agents
            .list()
            .context("read Hub Agent projection")?
            .into_iter()
            .filter(|record| !record.internal && record.archived_at.is_none())
            .map(|record| {
                json!({
                    "id": record.id,
                    "data": {
                        "title": record.title,
                        "workspaceId": record.workspace_id,
                        "provider": record.provider,
                        "status": record.last_status,
                        "requiresAttention": record.requires_attention,
                        "attentionReason": record.attention_reason,
                        "attentionTimestamp": record.attention_timestamp,
                        "createdAt": record.created_at,
                        "updatedAt": record.updated_at
                    }
                })
            })
            .collect::<Vec<_>>();
        for items in [&projects, &workspaces, &agents] {
            anyhow::ensure!(
                items.len() <= 1000,
                "Host directory exceeds center item budget"
            );
            for item in items {
                anyhow::ensure!(
                    serde_json::to_vec(&item["data"])?.len() <= 16 * 1024,
                    "Host directory item exceeds center budget"
                );
            }
        }
        Ok([
            ("project", projects.into()),
            ("workspace", workspaces.into()),
            ("agent", agents.into()),
        ])
    }

    pub(super) fn attention(&self, now: i64) -> anyhow::Result<Vec<Value>> {
        self.agents
            .list()
            .context("read current Host attention")?
            .into_iter()
            .filter(|record| {
                !record.internal && record.archived_at.is_none() && record.requires_attention
            })
            .filter_map(|record| {
                let timestamp = record.attention_timestamp?;
                let occurred = chrono::DateTime::parse_from_rfc3339(&timestamp)
                    .ok()?
                    .timestamp_millis();
                // Recover recent events without republishing old unread records daily.
                if now.saturating_sub(occurred) >= 24 * 60 * 60 * 1000 || occurred > now {
                    return None;
                }
                Some((
                    record.id,
                    record.workspace_id,
                    record.attention_reason?,
                    timestamp,
                ))
            })
            .map(|(id, workspace, reason, timestamp)| {
                Ok(json!({
                    "agentId":id,"workspaceId":workspace,"reason":reason,"timestamp":timestamp
                }))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
