//! Discards the v1 multi-Bot store, which is never loaded or migrated.

use std::collections::BTreeMap;
use std::path::Path;

use domain::bot::SessionOwner;
use domain::{BotId, ConversationId};
use provider::service::bot_execution::Action;
use serde_json::Value;

use crate::ports::Executor;

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn field<T: serde::de::DeserializeOwned>(record: &Value, name: &str) -> Option<T> {
    serde_json::from_value(record.get(name)?.clone()).ok()
}

/// Leniently list the hidden sessions recorded under a legacy store root.
///
/// Reads only `botId`, `workspaceId`, conversation `id` and `sessionId`; files that cannot be
/// parsed, and conversations whose Bot or Workspace cannot be resolved, are skipped.
#[must_use]
pub fn sessions(root: &Path) -> Vec<(SessionOwner, String)> {
    let workspaces: BTreeMap<BotId, String> = read_json(&root.join("catalog.json"))
        .and_then(|catalog| catalog.get("bots")?.as_array().cloned())
        .into_iter()
        .flatten()
        .filter_map(|bot| Some((field(&bot, "id")?, field(&bot, "workspaceId")?)))
        .collect();
    let Ok(entries) = std::fs::read_dir(root.join("conversations")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| {
            let record = read_json(&path)?;
            let bot_id: BotId = field(&record, "botId")?;
            let conversation_id: ConversationId = field(&record, "id")?;
            let session_id: String = field(&record, "sessionId")?;
            let workspace_id = workspaces.get(&bot_id)?.clone();
            Some((
                SessionOwner {
                    bot_id,
                    conversation_id,
                    workspace_id,
                },
                session_id,
            ))
        })
        .collect()
}

/// Close every session recorded in the legacy store, then delete the store directory.
///
/// Filesystem reads and the removal run on the blocking pool. If the store cannot be listed,
/// it is kept so its sessions are not orphaned. Failures are logged by count and error kind
/// only, never with conversation content.
pub async fn discard(root: &Path, executor: &dyn Executor) {
    let root = root.to_path_buf();
    let listed = root.clone();
    let Ok(sessions) = tokio::task::spawn_blocking(move || sessions(&listed)).await else {
        tracing::warn!("failed to list legacy Bot sessions");
        return;
    };
    let mut failed = 0_usize;
    for (owner, agent_id) in sessions {
        if executor
            .execute(owner, Action::Close { agent_id })
            .await
            .is_err()
        {
            failed += 1;
        }
    }
    if failed > 0 {
        tracing::warn!(failed, "failed to close legacy Bot sessions");
    }
    match tokio::task::spawn_blocking(move || std::fs::remove_dir_all(root)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(Err(error)) => {
            tracing::warn!(kind = ?error.kind(), "failed to remove legacy Bot store");
        }
        Err(_) => tracing::warn!("legacy Bot store removal did not finish"),
    }
}

#[cfg(test)]
mod tests;
