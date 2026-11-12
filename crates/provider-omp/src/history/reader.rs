//! Tolerant reader of one OMP session file (D17, O13), ported from Paseo's `history.ts`.
//!
//! The first pass indexes every entry's id, parent and byte range without keeping record
//! payloads; the second re-reads only the active branch. Malformed and partial lines are
//! skipped, a later duplicate id replaces an earlier one, and a dangling parent ends the
//! branch, as Paseo tolerates them.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use super::super::config;
use provider_core::ports::agent_session::AgentSessionError;

/// A session file larger than this fails instead of being read.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Entries indexed per file.
const MAX_ENTRIES: usize = 131_072;

/// The parts of a session file that history needs.
#[derive(Debug)]
pub(super) struct Snapshot {
    /// The first `session` record, with a valid `id` and `cwd`.
    pub(super) header: Value,
    /// The last nonblank native title: a `title` slot or a Pi `session_info` name.
    pub(super) title: Option<String>,
    /// The active branch, root first.
    pub(super) entries: Vec<Value>,
}

/// One line's graph facts; other fields are skipped without being materialized.
#[derive(Deserialize)]
struct Line<'a> {
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(default)]
    id: Option<String>,
    #[serde(rename = "parentId", default)]
    parent: Option<String>,
    #[serde(rename = "activeLeafId", default, deserialize_with = "leaf")]
    active: Leaf,
}

/// An entry's `activeLeafId`: OMP 18.18.8 records the branch it returned to on an entry
/// appended to another branch.
#[derive(Debug, Default, PartialEq, Eq)]
enum Leaf {
    /// No `activeLeafId`: the entry itself is on the active branch.
    #[default]
    Absent,
    /// An explicit null: the active branch is empty.
    Empty,
    /// The active branch ends at this entry.
    Entry(String),
}

fn leaf<'de, D>(deserializer: D) -> Result<Leaf, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(|leaf| leaf.map_or(Leaf::Empty, Leaf::Entry))
}

/// Where one indexed entry is in the file.
struct Location {
    id: String,
    parent: Option<String>,
    active: Leaf,
    offset: u64,
    length: usize,
}

/// Read `path`'s header, title and active branch.
///
/// # Errors
///
/// Returns unavailable for a missing file, and failed for a file that is not a regular file,
/// exceeds 64 MiB or 131072 entries, has no valid `session` header, or changed between passes.
pub(super) fn read(path: &Path) -> Result<Snapshot, AgentSessionError> {
    let metadata = path
        .symlink_metadata()
        .map_err(|_| AgentSessionError::Unavailable)?;
    if !metadata.is_file() {
        return Err(AgentSessionError::Failed);
    }
    let mut file = std::fs::File::open(path).map_err(|_| AgentSessionError::Unavailable)?;
    // A fixed boundary excludes entries appended while the file is read.
    let size = file
        .metadata()
        .map_err(|_| AgentSessionError::Failed)?
        .len();
    if size > MAX_BYTES {
        return Err(AgentSessionError::Failed);
    }
    let index = index(&mut file, size)?;
    let chain = chain(&index.locations);
    let entries = reread(&mut file, &index.locations, &chain)?;
    Ok(Snapshot {
        header: index.header.ok_or(AgentSessionError::Failed)?,
        title: index.title,
        entries,
    })
}

/// The first pass over a file.
struct Index {
    header: Option<Value>,
    title: Option<String>,
    locations: Vec<Location>,
}

fn index(file: &mut std::fs::File, size: u64) -> Result<Index, AgentSessionError> {
    let mut index = Index {
        header: None,
        title: None,
        locations: Vec::new(),
    };
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, Read::by_ref(file).take(size));
    let mut buffer = Vec::new();
    let mut offset = 0;
    loop {
        buffer.clear();
        let read = reader
            .read_until(b'\n', &mut buffer)
            .map_err(|_| AgentSessionError::Failed)?;
        if read == 0 {
            break;
        }
        let start = offset;
        offset += read as u64;
        let line = trim_line(&buffer);
        // OMP can leave a partial last record; malformed records are ignored, as in Paseo.
        let Ok(parsed) = serde_json::from_slice::<Line<'_>>(line) else {
            continue;
        };
        match parsed.kind.as_deref() {
            // The header names the session, not a branch entry.
            Some("session") => {
                if index.header.is_none() {
                    index.header = header(line);
                }
                continue;
            }
            Some(kind @ ("title" | "session_info")) => {
                if let Some(title) = title(line, kind) {
                    index.title = Some(title);
                }
            }
            _ => {}
        }
        let Some(id) = parsed.id else {
            continue;
        };
        if index.locations.len() >= MAX_ENTRIES {
            return Err(AgentSessionError::Failed);
        }
        index.locations.push(Location {
            id,
            parent: parsed.parent,
            active: parsed.active,
            offset: start,
            length: line.len(),
        });
    }
    Ok(index)
}

/// A line without its `\n` or `\r\n` ending.
fn trim_line(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// A `session` record with the identity and directory every reader relies on.
fn header(line: &[u8]) -> Option<Value> {
    let header: Value = serde_json::from_slice(line).ok()?;
    config::text(&header, "id").ok()?;
    config::text(&header, "cwd").ok()?;
    Some(header)
}

/// The nonblank title a `title` slot or a Pi `session_info` entry names.
pub(super) fn title(line: &[u8], kind: &str) -> Option<String> {
    let record: Value = serde_json::from_slice(line).ok()?;
    let field = if kind == "session_info" {
        "name"
    } else {
        "title"
    };
    record[field]
        .as_str()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
}

/// Indexes of the active branch, leaf first. The leaf is the last entry's `activeLeafId`
/// when it names a known entry (an explicit null means an empty branch), as OMP 18.18.8
/// writes it; otherwise the last entry that no entry names as its parent, as in Paseo.
fn chain(locations: &[Location]) -> Vec<usize> {
    let Some(last) = locations.last() else {
        return Vec::new();
    };
    let by_id: HashMap<&str, usize> = locations
        .iter()
        .enumerate()
        .map(|(position, location)| (location.id.as_str(), position))
        .collect();
    let leaf = match &last.active {
        Leaf::Empty => return Vec::new(),
        Leaf::Entry(id) => by_id.get(id.as_str()).copied(),
        Leaf::Absent => None,
    }
    .or_else(|| {
        let parents: HashSet<&str> = locations
            .iter()
            .filter_map(|location| location.parent.as_deref())
            .collect();
        locations
            .iter()
            .rposition(|location| !parents.contains(location.id.as_str()))
    })
    .unwrap_or(locations.len() - 1);
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    let mut next = Some(leaf);
    while let Some(position) = next {
        let location = &locations[position];
        if !seen.insert(location.id.as_str()) {
            break;
        }
        chain.push(position);
        next = location
            .parent
            .as_deref()
            .and_then(|parent| by_id.get(parent).copied());
    }
    chain
}

/// Read the `chain` entries again, root first. A line that no longer holds its indexed
/// entry means the file was rewritten in place, so no partial branch is returned.
fn reread(
    file: &mut std::fs::File,
    locations: &[Location],
    chain: &[usize],
) -> Result<Vec<Value>, AgentSessionError> {
    let mut entries = Vec::with_capacity(chain.len());
    let mut buffer = Vec::new();
    for &position in chain.iter().rev() {
        let location = &locations[position];
        buffer.resize(location.length, 0);
        file.seek(SeekFrom::Start(location.offset))
            .and_then(|_| file.read_exact(&mut buffer))
            .map_err(|_| AgentSessionError::Failed)?;
        let entry: Value =
            serde_json::from_slice(&buffer).map_err(|_| AgentSessionError::Failed)?;
        if entry["id"] != location.id.as_str() {
            return Err(AgentSessionError::Failed);
        }
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests;
