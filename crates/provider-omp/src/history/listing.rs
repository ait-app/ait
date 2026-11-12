//! Session listing from a bounded head and tail of each file (D19), as Paseo's
//! `session-descriptor.ts` reads them, instead of parsing whole files.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;

use super::super::{LABEL, PROVIDER, config};
use super::reader;
use provider::ports::native_history::SessionDescriptor;

/// Bytes read from the start of a file for its header, title and first prompt.
const HEAD_BYTES: u64 = 64 * 1024;
/// Bytes read from the end of a file for its title, last prompt and last activity.
const TAIL_BYTES: u64 = 256 * 1024;
/// Head lines parsed once a first prompt is known.
const HEAD_LINES: usize = 2_000;
/// Characters kept from a prompt preview, as Paseo keeps.
const PREVIEW_CHARACTERS: usize = 160;

/// A described session file: its descriptor when it is listed, and the bytes read.
pub(super) struct Listed {
    pub(super) descriptor: Option<SessionDescriptor>,
    pub(super) bytes: u64,
}

/// What the head of a session file reports.
#[derive(Default)]
struct Head {
    header: Option<Value>,
    title: Option<String>,
    first_prompt: Option<String>,
    conversation: bool,
}

/// What the tail of a session file reports, read from its end.
#[derive(Default)]
struct Tail {
    title: Option<String>,
    last_message: Option<String>,
    last_entry: Option<String>,
    last_prompt: Option<String>,
    conversation: bool,
}

/// Describe the session at `path` from its head and tail. A file without a valid header or
/// without any conversation message is not listed. `matches` filters by the header's `cwd`
/// before the tail is read. Returns none for a file that cannot be read.
pub(super) fn describe(path: &Path, matches: impl Fn(&Value) -> bool) -> Option<Listed> {
    let mut file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let size = metadata.len();
    let head = read_head(&mut file, size)?;
    let mut listed = Listed {
        descriptor: None,
        bytes: HEAD_BYTES.min(size),
    };
    let Some(header) = head
        .header
        .as_ref()
        .filter(|header| matches(&header["cwd"]))
    else {
        return Some(listed);
    };
    let tail_start = size.saturating_sub(TAIL_BYTES);
    listed.bytes += size - tail_start;
    let tail = read_range(&mut file, tail_start, size - tail_start)
        .map(|bytes| parse_tail(&bytes))
        .unwrap_or_default();
    if !head.conversation && !tail.conversation {
        return Some(listed);
    }
    let title = tail
        .title
        .or(head.title)
        .or_else(|| readable_name(path))
        .or_else(|| head.first_prompt.clone());
    let last_activity = tail
        .last_message
        .or(tail.last_entry)
        .or_else(|| {
            metadata.modified().ok().map(|time| {
                DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::AutoSi, true)
            })
        })
        .or_else(|| header["timestamp"].as_str().and_then(timestamp));
    let last_prompt = tail.last_prompt.or_else(|| head.first_prompt.clone());
    listed.descriptor = last_activity.map(|last_activity| SessionDescriptor {
        provider_id: PROVIDER.to_owned(),
        provider_label: LABEL.to_owned(),
        provider_handle_id: header["id"].as_str().unwrap_or_default().to_owned(),
        cwd: header["cwd"].as_str().unwrap_or_default().to_owned(),
        title,
        first_prompt_preview: head.first_prompt,
        last_prompt_preview: last_prompt,
        last_activity_at: last_activity,
    });
    Some(listed)
}

fn read_head(file: &mut std::fs::File, size: u64) -> Option<Head> {
    let bytes = read_range(file, 0, HEAD_BYTES.min(size))?;
    let mut head = Head::default();
    // OMP 18.18.8 writes its `title` slot before the session header.
    for (count, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        match entry["type"].as_str() {
            Some("session") if head.header.is_none() => {
                head.header = (config::text(&entry, "id").is_ok()
                    && config::text(&entry, "cwd").is_ok())
                .then_some(entry);
            }
            Some(kind @ ("title" | "session_info")) => {
                if let Some(title) = reader::title(line, kind) {
                    head.title = Some(title);
                }
            }
            Some("message" | "custom_message") => {
                head.conversation = true;
                if head.first_prompt.is_none() && entry["message"]["role"] == "user" {
                    head.first_prompt = preview(&entry["message"]["content"]);
                }
            }
            _ => {}
        }
        if count + 1 >= HEAD_LINES && head.first_prompt.is_some() {
            break;
        }
    }
    Some(head)
}

/// Scan a tail chunk from its end; its first, likely partial, line fails to parse.
fn parse_tail(bytes: &[u8]) -> Tail {
    let mut tail = Tail::default();
    for line in bytes.rsplit(|byte| *byte == b'\n') {
        let Ok(entry) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        let kind = entry["type"].as_str();
        if tail.title.is_none()
            && let Some(kind @ ("title" | "session_info")) = kind
        {
            tail.title = reader::title(line, kind);
        }
        let stamp = entry["timestamp"].as_str().and_then(timestamp);
        if tail.last_entry.is_none() {
            tail.last_entry.clone_from(&stamp);
        }
        if !matches!(kind, Some("message" | "custom_message")) {
            continue;
        }
        tail.conversation = true;
        if kind != Some("message") {
            continue;
        }
        if tail.last_message.is_none() {
            tail.last_message = stamp;
        }
        if tail.last_prompt.is_none() && entry["message"]["role"] == "user" {
            tail.last_prompt = preview(&entry["message"]["content"]);
        }
    }
    tail
}

fn read_range(file: &mut std::fs::File, start: u64, length: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::with_capacity(usize::try_from(length).ok()?);
    file.take(length).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// A whitespace-normalized prompt preview of at most 160 characters.
fn preview(content: &Value) -> Option<String> {
    let text = super::prompt_preview(content)?;
    Some(match text.char_indices().nth(PREVIEW_CHARACTERS) {
        Some((end, _)) => text[..end].to_owned(),
        None => text,
    })
}

/// An RFC 3339 timestamp in UTC, so descriptors sort by text.
fn timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| {
        time.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true)
    })
}

/// A file name that reads as a title, as Paseo's `readReadableSessionTitleFromPath`: not
/// OMP's `<timestamp>_<id>` name and not a bare hexadecimal id.
pub(super) fn readable_name(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.trim();
    let bytes = stem.as_bytes();
    let dated = bytes.len() >= 19
        && [0..4, 5..7, 8..10, 11..13, 14..16, 17..19]
            .into_iter()
            .all(|range| bytes[range].iter().all(u8::is_ascii_digit))
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && matches!(bytes[13], b'-' | b':')
        && matches!(bytes[16], b'-' | b':');
    let hexadecimal = bytes.len() >= 8 && bytes.iter().all(u8::is_ascii_hexdigit);
    (!stem.is_empty() && !dated && !hexadecimal).then(|| stem.to_owned())
}

#[cfg(test)]
mod tests;
