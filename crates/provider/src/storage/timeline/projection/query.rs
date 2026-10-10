//! Index seeks for display pages; oversized groups fall back to bounded normalized source reads.

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::protocol::timeline::{Direction, FetchRequest};
use crate::timeline_projection::page::{self, Header, Page};
use crate::timeline_projection::{Entry, project};

use super::super::{ErrorCode, Row, io};

/// Seek the first and last source positions of `agent`; returns `(0, 0)` for an empty scope.
/// Returns storage errors without parsing source or display entries.
pub(super) fn bounds(database: &Connection, agent: &str) -> Result<(u64, u64), ErrorCode> {
    database.query_row(
        "SELECT COALESCE((SELECT seq FROM display_sources WHERE agent=?1 ORDER BY seq LIMIT 1),0),
         COALESCE((SELECT seq FROM display_sources WHERE agent=?1 ORDER BY seq DESC LIMIT 1),0)",
        [agent], |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(io)
}

struct Read<'a> {
    database: &'a Connection,
    agent: &'a str,
    header: &'a Header,
    bytes: usize,
}

/// Read the window chosen by `request` and consistent `header` for `agent` within `bytes`.
/// Returns storage, derived-data decoding or exhausted response-budget errors.
pub(super) fn read(
    database: &Connection,
    agent: &str,
    request: &FetchRequest,
    header: &Header,
    bytes: usize,
) -> Result<Page, ErrorCode> {
    let read = Read {
        database,
        agent,
        header,
        bytes,
    };
    let cursor = request.cursor.as_ref().map(|cursor| cursor.seq);
    if header.max_seq == 0 {
        return Ok(Page {
            entries: Vec::new(),
            start_seq: None,
            end_seq: None,
            has_older: false,
            has_newer: false,
        });
    }
    match header.selection {
        Direction::Tail => read.backward(None),
        Direction::Before => read.backward(cursor),
        Direction::After => read.after(cursor),
    }
}

impl Read<'_> {
    fn backward(&self, cursor: Option<u64>) -> Result<Page, ErrorCode> {
        let end = cursor.map_or(self.header.max_seq, |seq| {
            seq.saturating_sub(1).min(self.header.max_seq)
        });
        let first = self
            .database
            .query_row(
                "SELECT seq_start FROM display_entries WHERE agent=? AND seq_start<=?
             ORDER BY seq_start LIMIT 1",
                params![self.agent, end],
                |row| row.get::<_, u64>(0),
            )
            .optional()
            .map_err(io)?;
        let Some(first) = first else {
            return Ok(Page {
                entries: Vec::new(),
                start_seq: None,
                end_seq: (end >= self.header.min_seq).then_some(end),
                has_older: false,
                has_newer: end < self.header.max_seq,
            });
        };
        let mut start = if self.header.limit == 0 {
            first
        } else {
            self.database
                .query_row(
                    "SELECT seq_start FROM display_entries WHERE agent=? AND seq_start<=?
                 ORDER BY seq_start DESC LIMIT 1 OFFSET ?",
                    params![self.agent, end, self.header.limit - 1],
                    |row| row.get::<_, u64>(0),
                )
                .optional()
                .map_err(io)?
                .unwrap_or(first)
        };
        if self.header.selection == Direction::Tail {
            loop {
                let earlier: Option<u64> = self
                    .database
                    .query_row(
                        "SELECT MIN(seq_start) FROM display_entries INDEXED BY display_end
                     WHERE agent=? AND seq_start<? AND seq_end>=?",
                        params![self.agent, start, start],
                        |row| row.get(0),
                    )
                    .map_err(io)?;
                let Some(earlier) = earlier else {
                    break;
                };
                start = earlier;
            }
        }
        let mut query = self.database.prepare(
            "SELECT entry,bytes FROM display_entries WHERE agent=? AND seq_start BETWEEN ? AND ?
             ORDER BY seq_start",
        ).map_err(io)?;
        let mut rows = query.query(params![self.agent, start, end]).map_err(io)?;
        let mut used = 2;
        let mut entries = Vec::new();
        while let Some(row) = rows.next().map_err(io)? {
            used += row.get::<_, usize>(1).map_err(io)? + usize::from(!entries.is_empty());
            if used > self.bytes {
                return self.sources(start, end);
            }
            let encoded: String = row.get(0).map_err(io)?;
            entries.push(serde_json::from_str(&encoded).map_err(io)?);
        }
        Ok(Page {
            entries,
            start_seq: Some(start),
            end_seq: Some(end),
            has_older: start > self.header.min_seq,
            has_newer: end < self.header.max_seq,
        })
    }

    fn after(&self, cursor: Option<u64>) -> Result<Page, ErrorCode> {
        let start = cursor.map_or(self.header.min_seq, |seq| {
            seq.saturating_add(1).max(self.header.min_seq)
        });
        let predecessor: Option<u64> = self
            .database
            .query_row(
                "SELECT start_seq FROM display_ranges WHERE agent=? AND start_seq<=?
             ORDER BY start_seq DESC LIMIT 1",
                params![self.agent, start],
                |row| row.get(0),
            )
            .optional()
            .map_err(io)?;
        let mut query = self
            .database
            .prepare(
                "SELECT start_seq,entry_seq FROM display_ranges
             WHERE agent=? AND start_seq>=? AND end_seq>=? ORDER BY start_seq",
            )
            .map_err(io)?;
        let mut ranges = query
            .query(params![self.agent, predecessor.unwrap_or(start), start])
            .map_err(io)?;
        let mut ids = BTreeSet::new();
        let mut used = 2;
        let mut end = self.header.max_seq;
        while let Some(range) = ranges.next().map_err(io)? {
            let id: u64 = range.get(1).map_err(io)?;
            if ids.contains(&id) {
                continue;
            }
            if self.header.limit > 0 && ids.len() == self.header.limit {
                end = range.get::<_, u64>(0).map_err(io)?.saturating_sub(1);
                break;
            }
            let bytes: usize = self
                .database
                .query_row(
                    "SELECT bytes FROM display_entries WHERE agent=? AND seq_start=?",
                    params![self.agent, id],
                    |row| row.get(0),
                )
                .map_err(io)?;
            used += bytes + usize::from(!ids.is_empty());
            ids.insert(id);
            if self.header.limit == 0 && used > self.bytes {
                return self.sources(start, self.header.max_seq);
            }
        }
        if used > self.bytes {
            return self.sources(start, end);
        }
        let entries = ids
            .into_iter()
            .map(|id| {
                let encoded: String = self
                    .database
                    .query_row(
                        "SELECT entry FROM display_entries WHERE agent=? AND seq_start=?",
                        params![self.agent, id],
                        |row| row.get(0),
                    )
                    .map_err(io)?;
                serde_json::from_str::<Entry>(&encoded).map_err(io)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(page::after(
            entries,
            self.header.min_seq,
            self.header.max_seq,
            cursor,
            0,
        ))
    }

    fn sources(&self, start: u64, end: u64) -> Result<Page, ErrorCode> {
        let order = if self.header.selection == Direction::After {
            "ASC"
        } else {
            "DESC"
        };
        let mut query = self
            .database
            .prepare(&format!(
                "SELECT seq,provider,entry,bytes FROM display_sources
             WHERE agent=? AND seq BETWEEN ? AND ? ORDER BY seq {order}",
            ))
            .map_err(io)?;
        let mut sources = query.query(params![self.agent, start, end]).map_err(io)?;
        let mut rows = Vec::new();
        let mut used = 2;
        while let Some(source) = sources.next().map_err(io)? {
            let next =
                used + source.get::<_, usize>(3).map_err(io)? + usize::from(!rows.is_empty());
            if next > self.bytes {
                break;
            }
            used = next;
            let encoded: String = source.get(2).map_err(io)?;
            rows.push(Row {
                seq: source.get(0).map_err(io)?,
                provider: source.get(1).map_err(io)?,
                entry: serde_json::from_str(&encoded).map_err(io)?,
            });
        }
        if rows.is_empty() {
            return Err(ErrorCode::ResourceExhausted);
        }
        if self.header.selection != Direction::After {
            rows.reverse();
        }
        let first = rows.first().ok_or(ErrorCode::ResourceExhausted)?.seq;
        let last = rows.last().ok_or(ErrorCode::ResourceExhausted)?.seq;
        let page = Page {
            entries: project(&rows),
            start_seq: Some(first),
            end_seq: Some(last),
            has_older: first > self.header.min_seq,
            has_newer: last < self.header.max_seq,
        };
        page::fit(&rows, page, self.header.selection, self.bytes)
    }
}
