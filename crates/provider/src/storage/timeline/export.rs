//! Whole-snapshot export reads canonical saved rows, never native history or a visible UI page.

use std::collections::BTreeMap;

use model::ErrorCode;
use model::download::MAX_ARTIFACT_BYTES;
use rusqlite::params;
use tokio_util::sync::CancellationToken;

use super::{NativeItem, Row, Timeline, io};

const PAGE_ROWS: usize = 128;
const MAX_SOURCE_ROWS: usize = 20_000;

impl Timeline {
    /// Read every persisted source page in one SQLite snapshot, normalizing saved progress exactly
    /// as normal history does. Missing history is an empty export, never a native-history refresh.
    /// No generation, source row, or derived index is created by this read.
    /// # Errors
    /// Returns an explicit error on cancellation, malformed storage, or the 16-MiB/20,000-row bound;
    /// it never returns a truncated prefix. Rendering occurs after the database lock is released.
    pub(crate) fn export_rows(
        &self,
        agent: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<Row>, ErrorCode> {
        let mut database = self.database.lock().map_err(io)?;
        let transaction = database.transaction().map_err(io)?;
        let mut sources = Vec::new();
        let mut prefixes = BTreeMap::new();
        let mut bytes = 0_u64;
        let mut cursor = 0_u64;
        loop {
            if cancellation.is_cancelled() {
                return Err(ErrorCode::AgentIo);
            }
            let mut query = transaction
                .prepare(
                    "SELECT seq,provider,entry,0 FROM entries WHERE agent=?1 AND seq>?2
                 UNION ALL
                 SELECT seq,provider,entry,1 FROM progress WHERE agent=?1 AND seq>?2
                 ORDER BY seq LIMIT ?3",
                )
                .map_err(io)?;
            let mut page = query.query(params![agent, cursor, PAGE_ROWS]).map_err(io)?;
            let mut read = 0;
            while let Some(row) = page.next().map_err(io)? {
                if cancellation.is_cancelled() {
                    return Err(ErrorCode::AgentIo);
                }
                let encoded = row.get_ref(2).map_err(io)?.as_str().map_err(io)?;
                let provider = row.get_ref(1).map_err(io)?.as_str().map_err(io)?;
                bytes = bytes
                    .saturating_add(encoded.len() as u64)
                    .saturating_add(provider.len() as u64);
                if bytes > MAX_ARTIFACT_BYTES || sources.len() == MAX_SOURCE_ROWS {
                    return Err(ErrorCode::ResourceExhausted);
                }
                let mut entry: NativeItem = serde_json::from_str(encoded).map_err(io)?;
                super::progress::normalize(&mut entry, row.get(3).map_err(io)?, &mut prefixes)?;
                cursor = row.get(0).map_err(io)?;
                sources.push(Row {
                    seq: cursor,
                    provider: provider.to_owned(),
                    entry,
                });
                read += 1;
            }
            if read < PAGE_ROWS {
                break;
            }
        }
        transaction.commit().map_err(io)?;
        Ok(sources)
    }
}

#[cfg(test)]
mod tests;
