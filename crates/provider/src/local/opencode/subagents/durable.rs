//! Read only native completion facts for children whose task link is already verified by ACP.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::json;

use crate::{
    local::opencode::client::OpenCodeClient,
    ports::{agent_session::AgentSessionError, controls::NativeSubagent},
};

struct OutcomeRow {
    parent: String,
    cwd: String,
    busy: Option<i64>,
    idle: Option<i64>,
    outcome: Option<String>,
}

/// Mirror `OpenCode`'s database location: relative overrides resolve under its data directory.
fn database_path(client: &OpenCodeClient) -> Option<PathBuf> {
    let variable = |name: &str| {
        client
            .environment
            .get(name)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os(name).map(PathBuf::from))
    };
    let file = variable("OPENCODE_DB").unwrap_or_else(|| PathBuf::from("opencode.db"));
    if file.as_os_str() == ":memory:" {
        return None;
    }
    if file.is_absolute() {
        return Some(file);
    }
    let data = variable("XDG_DATA_HOME")
        .or_else(|| variable("HOME").map(|home| home.join(".local/share")))?;
    Some(data.join("opencode").join(file))
}

fn durable_status(
    connection: &Connection,
    child: &NativeSubagent,
) -> rusqlite::Result<Option<&'static str>> {
    let row: Option<OutcomeRow> = connection
        .query_row(
            "SELECT parent_id,directory,time_busy,time_idle,idle_outcome FROM session_v2 WHERE id=?1",
            params![child.id],
            |row| Ok(OutcomeRow {
                parent: row.get(0)?,
                cwd: row.get(1)?,
                busy: row.get(2)?,
                idle: row.get(3)?,
                outcome: row.get(4)?,
            }),
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.parent != child.parent_id
        || row.cwd != child.cwd
        || row
            .busy
            .zip(row.idle)
            .is_none_or(|(busy, idle)| idle < busy)
    {
        return Ok(None);
    }
    Ok(match row.outcome.as_deref() {
        Some("succeeded") => Some("completed"),
        Some("failed") => Some("failed"),
        Some("interrupted") => Some("canceled"),
        _ => None,
    })
}

fn reconcile_from_path(path: PathBuf, mut children: Vec<NativeSubagent>) -> Vec<NativeSubagent> {
    let Ok(connection) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return children;
    };
    let now = chrono::Utc::now().to_rfc3339();
    for child in &mut children {
        if child.descriptor["status"] != "running" {
            continue;
        }
        if let Ok(Some(status)) = durable_status(&connection, child) {
            child.descriptor["status"] = json!(status);
            child.descriptor["updatedAt"] = json!(now);
        }
    }
    children
}

/// Reconcile verified `OpenCode` children against durable V2 terminal outcomes without writing storage.
/// # Errors
/// Returns a worker failure. An absent database or older schema retains the ACP observations.
pub(in crate::local::opencode) async fn reconcile(
    client: &OpenCodeClient,
    children: Vec<NativeSubagent>,
) -> Result<Vec<NativeSubagent>, AgentSessionError> {
    if !children
        .iter()
        .any(|child| child.descriptor["status"] == "running")
    {
        return Ok(children);
    }
    let Some(path) = database_path(client) else {
        return Ok(children);
    };
    tokio::task::spawn_blocking(move || reconcile_from_path(path, children))
        .await
        .map_err(|_| AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
