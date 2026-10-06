//! Display-count limits with contiguous canonical source coverage.

use model::ErrorCode;
use serde::Serialize;

use crate::protocol::timeline::Direction;
use crate::storage::timeline::Row;

use super::{Entry, project};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
/// Projected page with source cursors that certify contiguous coverage.
pub(in crate::rpc::timeline) struct Page {
    pub(in crate::rpc::timeline) entries: Vec<Entry>,
    pub(in crate::rpc::timeline) start_seq: Option<u64>,
    pub(in crate::rpc::timeline) end_seq: Option<u64>,
    pub(in crate::rpc::timeline) has_older: bool,
    pub(in crate::rpc::timeline) has_newer: bool,
}

/// Select up to `limit` display entries relative to `cursor`; zero selects the full window.
pub(in crate::rpc::timeline) fn select(
    rows: &[Row],
    direction: Direction,
    cursor: Option<u64>,
    limit: usize,
) -> Page {
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Page {
            entries: Vec::new(),
            start_seq: None,
            end_seq: None,
            has_older: false,
            has_newer: false,
        };
    };
    let projected = project(rows);
    match direction {
        Direction::Tail => tail(projected, last.seq, limit),
        Direction::Before => before(projected, first.seq, last.seq, cursor, limit),
        Direction::After => after(projected, first.seq, last.seq, cursor, limit),
    }
}

/// Fit a page within its serialized entry budget without truncating individual payloads.
/// Oversized projections are split at source rows so interleaved tool lifecycles remain pageable.
/// # Errors
/// Returns a resource error when even one source row cannot fit, or an I/O serialization error.
pub(in crate::rpc::timeline) fn fit(
    rows: &[Row],
    mut page: Page,
    direction: Direction,
    bytes: usize,
) -> Result<Page, ErrorCode> {
    let mut encoded = Vec::new();
    let mut projected_bytes = 2;
    for (index, entry) in page.entries.iter().enumerate() {
        encoded.clear();
        serde_json::to_writer(&mut encoded, entry).map_err(|_| ErrorCode::AgentIo)?;
        projected_bytes += encoded.len() + usize::from(index > 0);
        if projected_bytes > bytes {
            break;
        }
    }
    if projected_bytes <= bytes {
        return Ok(page);
    }
    let start_seq = page.start_seq.ok_or(ErrorCode::ResourceExhausted)?;
    let end_seq = page.end_seq.ok_or(ErrorCode::ResourceExhausted)?;
    let start = rows.partition_point(|row| row.seq < start_seq);
    let end = rows.partition_point(|row| row.seq <= end_seq);
    let window = &rows[start..end];
    page.entries.clear();
    let mut used = 2; // JSON array brackets; each subsequent entry also needs a comma.
    let mut count = 0;
    for offset in 0..window.len() {
        let index = if direction == Direction::After {
            offset
        } else {
            window.len() - 1 - offset
        };
        encoded.clear();
        serde_json::to_writer(&mut encoded, &Entry::from(&window[index]))
            .map_err(|_| ErrorCode::AgentIo)?;
        let next = used + encoded.len() + usize::from(count > 0);
        if next > bytes {
            break;
        }
        used = next;
        count += 1;
    }
    if count == 0 {
        return Err(ErrorCode::ResourceExhausted);
    }
    let selected = if direction == Direction::After {
        &window[..count]
    } else {
        &window[window.len() - count..]
    };
    let first = selected.first().ok_or(ErrorCode::ResourceExhausted)?;
    let last = selected.last().ok_or(ErrorCode::ResourceExhausted)?;
    page.entries = project(selected);
    page.start_seq = Some(first.seq);
    page.end_seq = Some(last.seq);
    page.has_older |= first.seq > start_seq;
    page.has_newer |= last.seq < end_seq;
    Ok(page)
}

fn tail(mut entries: Vec<Entry>, max_seq: u64, limit: usize) -> Page {
    let mut start = if limit == 0 {
        0
    } else {
        entries.len().saturating_sub(limit)
    };
    for index in (0..start).rev() {
        if entries[index].seq_end >= entries[start].seq_start {
            start = index;
        }
    }
    let entries = entries.split_off(start);
    Page {
        start_seq: entries.first().map(|entry| entry.seq_start),
        entries,
        end_seq: Some(max_seq),
        has_older: start > 0,
        has_newer: false,
    }
}

fn before(
    mut entries: Vec<Entry>,
    min_seq: u64,
    max_seq: u64,
    cursor: Option<u64>,
    limit: usize,
) -> Page {
    let end_seq = cursor.map_or(max_seq, |cursor| cursor.saturating_sub(1).min(max_seq));
    entries.retain(|entry| entry.seq_start <= end_seq);
    let start = if limit == 0 {
        0
    } else {
        entries.len().saturating_sub(limit)
    };
    let entries = entries.split_off(start);
    Page {
        start_seq: entries.first().map(|entry| entry.seq_start),
        entries,
        end_seq: (end_seq >= min_seq).then_some(end_seq),
        has_older: start > 0,
        has_newer: end_seq < max_seq,
    }
}

fn after(
    entries: Vec<Entry>,
    min_seq: u64,
    max_seq: u64,
    cursor: Option<u64>,
    limit: usize,
) -> Page {
    let start_seq = cursor.map_or(min_seq, |cursor| cursor.saturating_add(1).max(min_seq));
    let mut eligible: Vec<_> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let first = entry.source_seq_ranges.iter().find_map(|range| {
                let first = range.start_seq.max(start_seq);
                (first <= range.end_seq.min(max_seq)).then_some(first)
            })?;
            Some((index, first, entry))
        })
        .collect();
    eligible.sort_unstable_by_key(|(index, first, _)| (*first, *index));
    if limit > 0 {
        eligible.truncate(limit);
    }
    eligible.sort_unstable_by_key(|(index, _, _)| *index);
    let entries: Vec<_> = eligible.into_iter().map(|(_, _, entry)| entry).collect();
    let mut ranges: Vec<_> = entries
        .iter()
        .flat_map(|entry| &entry.source_seq_ranges)
        .collect();
    ranges.sort_unstable_by_key(|range| (range.start_seq, range.end_seq));
    let mut end_seq = start_seq.saturating_sub(1);
    for range in ranges {
        if range.end_seq <= end_seq {
            continue;
        }
        if range.start_seq > end_seq.saturating_add(1) {
            break;
        }
        end_seq = range.end_seq.min(max_seq);
    }
    let end_seq = (end_seq >= start_seq).then_some(end_seq);
    Page {
        entries,
        start_seq: end_seq.map(|_| start_seq),
        end_seq,
        has_older: start_seq > min_seq,
        has_newer: end_seq.is_some_and(|end| end < max_seq),
    }
}
