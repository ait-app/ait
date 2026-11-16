//! Durable display descriptors for native tasks whose output can outlive their parent turn.

use rusqlite::{OptionalExtension, params};

use super::{ErrorCode, Timeline, io};
use provider_core::ports::controls::NativeSubagent;

impl Timeline {
    /// Return the canonical host identity previously bound to a verified native child.
    /// Returns storage errors without treating an unreadable binding as a missing child.
    pub(crate) fn subagent_agent(
        &self,
        parent: &str,
        child: &str,
    ) -> Result<Option<String>, ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        database
            .query_row(
                "SELECT host_agent FROM provider_subagents WHERE agent=? AND child=?",
                params![parent, child],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(io)
    }
    /// Bind a legacy child scope to its stable host identity without moving timeline rows.
    /// Returns a conflict when a different host identity already owns this scope.
    pub(crate) fn bind_subagent(
        &self,
        parent: &str,
        child: &str,
        agent: &str,
        observed: bool,
    ) -> Result<(), ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        let changed = database.execute(
            "UPDATE provider_subagents SET host_agent=?3,host_observed=?4 WHERE agent=?1 AND child=?2 AND (host_agent IS NULL OR (host_agent=?3 AND host_observed=?4))",
            params![parent, child, agent, observed],
        ).map_err(io)?;
        if changed != 1 {
            return Err(ErrorCode::IdempotencyConflict);
        }
        Ok(())
    }

    /// Read running native children and their host parent in one consistent snapshot.
    /// Returns storage errors without exposing a partial activity list.
    pub(crate) fn running_subagent_activity(
        &self,
    ) -> Result<Vec<(String, Option<String>)>, ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        let mut query = database.prepare(
            "SELECT agent,json_extract(descriptor,'$.descriptor.updatedAt') FROM provider_subagents
             WHERE json_extract(descriptor,'$.descriptor.status')='running' ORDER BY agent,child",
        ).map_err(io)?;
        query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(io)?
            .map(|row| row.map_err(io))
            .collect()
    }

    pub(crate) fn store_subagent(
        &self,
        parent: &str,
        child: &NativeSubagent,
    ) -> Result<(), ErrorCode> {
        if child.id.is_empty()
            || child.id.len() > 256
            || child.parent_id.is_empty()
            || child.descriptor["id"] != child.id
            || child.cwd.len() > 16384
        {
            return Err(ErrorCode::InvalidMessage);
        }
        let encoded = serde_json::to_string(child).map_err(io)?;
        if encoded.len() > 65536 {
            return Err(ErrorCode::ResourceExhausted);
        }
        let mut database = self.database.lock().map_err(io)?;
        let tx = database.transaction().map_err(io)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT descriptor FROM provider_subagents WHERE agent=? AND child=?",
                params![parent, child.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(io)?;
        if let Some(previous) = previous {
            let previous: NativeSubagent = serde_json::from_str(&previous).map_err(io)?;
            if previous.parent_id != child.parent_id {
                return Err(ErrorCode::IdempotencyConflict);
            }
            if let Some((previous_at, next_at)) = previous.descriptor["updatedAt"]
                .as_str()
                .zip(child.descriptor["updatedAt"].as_str())
                .and_then(|(previous, next)| {
                    Some((
                        chrono::DateTime::parse_from_rfc3339(previous).ok()?,
                        chrono::DateTime::parse_from_rfc3339(next).ok()?,
                    ))
                })
                && previous_at > next_at
            {
                return Ok(());
            }
        } else {
            let count: u64 = tx
                .query_row(
                    "SELECT count(*) FROM provider_subagents WHERE agent=?",
                    [parent],
                    |row| row.get(0),
                )
                .map_err(io)?;
            if count >= 4096 {
                return Err(ErrorCode::ResourceExhausted);
            }
        }
        tx.execute("INSERT INTO provider_subagents(agent,child,descriptor) VALUES(?,?,?) ON CONFLICT(agent,child) DO UPDATE SET descriptor=excluded.descriptor",
            params![parent,child.id,encoded]).map_err(io)?;
        tx.commit().map_err(io)?;
        Ok(())
    }

    pub(crate) fn subagents(&self, parent: &str) -> Result<Vec<NativeSubagent>, ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        let mut query = database
            .prepare("SELECT descriptor FROM provider_subagents WHERE agent=? ORDER BY child")
            .map_err(io)?;
        query
            .query_map([parent], |row| row.get::<_, String>(0))
            .map_err(io)?
            .map(|row| serde_json::from_str(&row.map_err(io)?).map_err(io))
            .collect()
    }
}

pub(super) fn binding(
    database: &rusqlite::Connection,
    scope: &str,
) -> Result<Option<String>, ErrorCode> {
    let Some((parent, child)) = scope
        .strip_prefix("subagent:")
        .and_then(|scope| scope.split_once(':'))
    else {
        return Ok(None);
    };
    database
        .query_row(
            "SELECT host_agent FROM provider_subagents WHERE agent=? AND child=? AND host_observed=1",
            params![parent, child],
            |row| row.get(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(io)
}

#[cfg(test)]
mod tests;
