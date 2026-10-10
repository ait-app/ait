//! Transactional, rebuildable display groups and source windows for bounded page reads.

use rusqlite::{Connection, OptionalExtension, params};

use crate::protocol::timeline::FetchRequest;
use crate::timeline_projection::page::{Header, Page};
use crate::timeline_projection::{Entry, identity, merge_adjacent, merge_identity};

use super::{ErrorCode, Row, Timeline, epoch, io};

mod query;

// Oversized text groups always use the bounded source fallback. Avoid rewriting their entire
// accumulated text for every delta; the complete source text stays in display_sources.
const MAX_GROUP_BYTES: usize = 900 * 1024;

impl Timeline {
    /// Read one display page in a consistent generation, with an envelope-derived byte budget.
    /// Legacy databases build their derived index once; subsequent reads and reopenings reuse it.
    /// `scope` selects an Agent or child timeline; `request` sets display limits and cursors.
    /// `budget` receives the consistent header and returns the remaining response bytes.
    /// # Errors
    /// Returns storage, malformed derived-data or response-budget errors.
    pub(crate) fn page(
        &self,
        request: &FetchRequest,
        scope: &str,
        budget: impl FnOnce(&Header) -> Result<usize, ErrorCode>,
    ) -> Result<Page, ErrorCode> {
        let mut database = self.database.lock().map_err(io)?;
        let transaction = database.transaction().map_err(io)?;
        ensure(&transaction, scope)?;
        let header = Header::new(
            request,
            epoch(&transaction, scope)?,
            query::bounds(&transaction, scope)?,
        );
        let bytes = budget(&header)?;
        let page = query::read(&transaction, scope, request, &header, bytes)?;
        transaction.commit().map_err(io)?;
        Ok(page)
    }
}

/// Remove derived state for `agent` on the caller's transaction; canonical rows stay intact.
/// Returns storage errors when a derived table cannot be cleared.
pub(super) fn clear(database: &Connection, agent: &str) -> Result<(), ErrorCode> {
    for table in [
        "projection_heads",
        "display_entries",
        "display_ranges",
        "display_sources",
    ] {
        database
            .execute(&format!("DELETE FROM {table} WHERE agent=?"), [agent])
            .map_err(io)?;
    }
    Ok(())
}

/// Rebuild `agent`'s derived state only if its persisted epoch differs from canonical storage.
/// Uses the caller's transaction and returns storage or source decoding errors.
pub(super) fn ensure(database: &Connection, agent: &str) -> Result<(), ErrorCode> {
    let epoch = epoch(database, agent)?;
    let indexed: Option<String> = database
        .query_row(
            "SELECT epoch FROM projection_heads WHERE agent=?",
            [agent],
            |row| row.get(0),
        )
        .optional()
        .map_err(io)?;
    if indexed.as_deref() == Some(epoch.as_str()) {
        return Ok(());
    }
    clear(database, agent)?;
    for row in super::progress::read(database, agent)? {
        append(database, agent, &row)?;
    }
    database
        .execute(
            "INSERT INTO projection_heads VALUES(?,?)",
            params![agent, epoch],
        )
        .map_err(io)?;
    Ok(())
}

/// Incrementally project a normalized source `row` for `agent` in the caller's transaction.
/// Returns storage or derived-entry decoding errors; transaction rollback undoes all changes.
pub(super) fn append(database: &Connection, agent: &str, row: &Row) -> Result<(), ErrorCode> {
    let incoming = Entry::from(row);
    let key = identity(&incoming.item);
    let previous = if let Some(key) = &key {
        database
            .query_row(
                "SELECT entry,bytes FROM display_entries WHERE agent=? AND identity=?
             ORDER BY seq_start DESC LIMIT 1",
                params![agent, key],
                stored,
            )
            .optional()
            .map_err(io)?
    } else {
        database.query_row(
            "SELECT entry,bytes FROM display_entries WHERE agent=? ORDER BY seq_start DESC LIMIT 1",
            [agent], stored,
        ).optional().map_err(io)?
    };
    let mut entry = incoming.clone();
    let mut oversized = false;
    if let Some((encoded, bytes)) = previous {
        let mut previous: Entry = serde_json::from_str(&encoded).map_err(io)?;
        let merged = if key.is_some() {
            merge_identity(&mut previous, &incoming)
        } else {
            oversized = bytes > MAX_GROUP_BYTES;
            let mut fragment = incoming.clone();
            if oversized {
                fragment.item["text"] = serde_json::json!("");
            }
            merge_adjacent(&mut previous, &fragment)
        };
        if merged {
            entry = previous;
        } else {
            oversized = false;
        }
    }
    let encoded = serde_json::to_string(&entry).map_err(io)?;
    let bytes = encoded.len();
    let encoded = if (oversized || bytes > MAX_GROUP_BYTES)
        && matches!(
            entry.item["type"].as_str(),
            Some("assistant_message" | "reasoning")
        ) {
        oversized = true;
        entry.item["text"] = serde_json::json!("");
        serde_json::to_string(&entry).map_err(io)?
    } else {
        encoded
    };
    database
        .execute(
            "INSERT INTO display_entries VALUES(?,?,?,?,?,?)
         ON CONFLICT(agent,seq_start) DO UPDATE SET seq_end=excluded.seq_end,
             entry=excluded.entry,bytes=excluded.bytes",
            params![
                agent,
                entry.seq_start,
                entry.seq_end,
                key,
                encoded,
                if oversized {
                    MAX_GROUP_BYTES + 1
                } else {
                    bytes
                }
            ],
        )
        .map_err(io)?;
    let source_bytes = serde_json::to_string(&incoming).map_err(io)?.len();
    database
        .execute(
            "INSERT INTO display_sources VALUES(?,?,?,?,?)",
            params![
                agent,
                row.seq,
                row.provider,
                serde_json::to_string(&row.entry).map_err(io)?,
                source_bytes
            ],
        )
        .map_err(io)?;
    append_range(database, agent, row.seq, entry.seq_start)
}

fn append_range(
    database: &Connection,
    agent: &str,
    seq: u64,
    entry_seq: u64,
) -> Result<(), ErrorCode> {
    let extended = database
        .execute(
            "UPDATE display_ranges SET end_seq=? WHERE agent=?
         AND start_seq=(SELECT MAX(start_seq) FROM display_ranges WHERE agent=? )
         AND end_seq=? AND entry_seq=?",
            params![seq, agent, agent, seq.saturating_sub(1), entry_seq],
        )
        .map_err(io)?;
    if extended == 0 {
        database
            .execute(
                "INSERT INTO display_ranges VALUES(?,?,?,?)",
                params![agent, seq, seq, entry_seq],
            )
            .map_err(io)?;
    }
    Ok(())
}

fn stored(row: &rusqlite::Row<'_>) -> rusqlite::Result<(String, usize)> {
    Ok((row.get(0)?, row.get(1)?))
}

#[cfg(test)]
mod tests;
