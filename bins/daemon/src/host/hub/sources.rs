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
            .filter(|record| !record.is_archived())
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
            .filter(|record| !record.is_archived())
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
            .snapshot()
            .context("read Hub Agent projection")?
            .iter()
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
        Ok([
            ("project", fit_budget("project", projects)),
            ("workspace", fit_budget("workspace", workspaces)),
            ("agent", fit_budget("agent", agents)),
        ])
    }

    pub(super) fn attention(&self, now: i64) -> anyhow::Result<Vec<Value>> {
        Ok(self
            .agents
            .snapshot()
            .context("read current Host attention")?
            .iter()
            .filter(|record| {
                !record.internal && record.archived_at.is_none() && record.requires_attention
            })
            .filter_map(|record| {
                let timestamp = record.attention_timestamp.as_deref()?;
                let occurred = chrono::DateTime::parse_from_rfc3339(timestamp)
                    .ok()?
                    .timestamp_millis();
                // Recover recent events without republishing old unread records daily.
                if now.saturating_sub(occurred) >= 24 * 60 * 60 * 1000 || occurred > now {
                    return None;
                }
                Some(json!({
                    "agentId":record.id,"workspaceId":record.workspace_id,
                    "reason":record.attention_reason?,"timestamp":timestamp
                }))
            })
            .collect())
    }
}

/// Most items the center accepts in one directory kind.
const ITEM_LIMIT: usize = 1000;
/// Largest encoded `data` object of one directory item.
const ITEM_BYTES: usize = 16 * 1024;
/// Encoded item budget per kind, leaving room for the publication envelope under 1 MiB.
const KIND_BYTES: usize = 1024 * 1024 - 1024;

/// Keep `items` within the center's directory budget.
///
/// A list that fits is returned unchanged. Otherwise oversized items are skipped and the most
/// recently updated items are kept, so an unusually large Host still publishes a useful partial
/// directory instead of failing every reconciliation.
fn fit_budget(kind: &str, items: Vec<Value>) -> Value {
    let mut sized = items
        .into_iter()
        .map(|item| {
            let size = (encoded_len(&item), encoded_len(&item["data"]));
            (item, size)
        })
        .collect::<Vec<_>>();
    let total = sized.iter().fold(2_usize, |total, (_, (encoded, _))| {
        total.saturating_add(encoded.saturating_add(1))
    });
    let fits = sized.len() <= ITEM_LIMIT
        && total <= KIND_BYTES
        && sized.iter().all(|(_, (_, data))| *data <= ITEM_BYTES);
    if !fits {
        sized.sort_by(|(left, _), (right, _)| {
            right["data"]["updatedAt"]
                .as_str()
                .cmp(&left["data"]["updatedAt"].as_str())
        });
        let mut bytes = 2_usize;
        sized.retain(|(_, (encoded, data))| {
            let next = bytes.saturating_add(encoded.saturating_add(1));
            let keep = *data <= ITEM_BYTES && next <= KIND_BYTES;
            if keep {
                bytes = next;
            }
            keep
        });
        sized.truncate(ITEM_LIMIT);
        tracing::info!(
            kind,
            published = sized.len(),
            "Host directory exceeds center budget; publishing the most recently updated items"
        );
    }
    sized.into_iter().map(|(item, _)| item).collect()
}

/// Encoded JSON length; a `Value` always serializes, so failure only marks the item oversized.
fn encoded_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests;
