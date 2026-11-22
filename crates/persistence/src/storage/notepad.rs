//! Workspace notepads as Markdown files under the daemon data directory (ADR-297).
//!
//! Each Workspace has `<root>/<encoded workspace id>/notepad.md`. Clients open the file with the
//! file methods, so it is plain text that is written by replacing the file.

use std::fs;
use std::io::{self, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use domain::workspace::notepad::{MAX_TEXT_BYTES, Notepad};
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use sha2::{Digest, Sha256};

const FILE_NAME: &str = "notepad.md";
/// Largest notepad file read; the file editor writes no text file beyond it.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Longest Workspace identity kept as a directory name, which is at most three times as long.
const MAX_ID_BYTES: usize = 80;
const BOM: &str = "\u{feff}";

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("notepad file I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("the workspace identity cannot name a notepad directory")]
    Identity,
    #[error("the notepad file is larger than 1 MiB or is not UTF-8 text")]
    Content,
}

impl From<StoreError> for NotepadStoreError {
    fn from(error: StoreError) -> Self {
        tracing::error!(%error, "notepad store operation failed");
        match error {
            StoreError::Io(error) if error.kind() == ErrorKind::StorageFull => Self::Full,
            _ => Self::Io,
        }
    }
}

/// Every Workspace's notepad file under one directory.
#[derive(Debug)]
pub struct FileNotepadStore {
    root: PathBuf,
}

impl FileNotepadStore {
    /// Keep notepad files under `root`, which is created with the first notepad.
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn directory(&self, workspace_id: &str) -> Result<PathBuf, StoreError> {
        if workspace_id.is_empty() || workspace_id.len() > MAX_ID_BYTES {
            return Err(StoreError::Identity);
        }
        Ok(self.root.join(encode(workspace_id)))
    }
}

/// Spell `workspace_id` as one portable file name: lowercase ASCII letters, digits, `_` and `-`
/// stand for themselves and every other byte is `%XX`, so names stay distinct on
/// case-insensitive volumes and never form `.` or `..`.
fn encode(workspace_id: &str) -> String {
    use std::fmt::Write as _;
    workspace_id.bytes().fold(
        String::with_capacity(workspace_id.len()),
        |mut name, byte| {
            if matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-') {
                name.push(char::from(byte));
            } else {
                let _ = write!(name, "%{byte:02X}");
            }
            name
        },
    )
}

/// The Workspace identity `name` spells, when [`encode`] produced it.
fn decode(name: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(name.len());
    let mut rest = name.as_bytes();
    while let [first, tail @ ..] = rest {
        if *first == b'%' {
            let (hex, tail) = tail.split_at_checked(2)?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = tail;
        } else {
            bytes.push(*first);
            rest = tail;
        }
    }
    String::from_utf8(bytes)
        .ok()
        .filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES && encode(id) == name)
}

/// Token that differs whenever the text does.
fn revision(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    format!(
        "{:016x}",
        u64::from_be_bytes(digest.first_chunk().map_or([0; 8], |head| *head))
    )
}

/// The notepad holding `text` in the file at `path` described by `stats`.
fn snapshot(path: &Path, text: String, stats: &fs::Metadata) -> Result<Notepad, StoreError> {
    let revision = revision(&text);
    let updated_at = DateTime::<Utc>::from(stats.modified()?);
    Ok(Notepad::new(
        path.to_string_lossy().into_owned(),
        text,
        revision,
        updated_at.to_rfc3339_opts(SecondsFormat::Millis, true),
    ))
}

fn read(path: &Path) -> Result<Notepad, StoreError> {
    let mut file = fs::File::open(path)?;
    let stats = file.metadata()?;
    if !stats.is_file() || stats.len() > MAX_FILE_BYTES {
        return Err(StoreError::Content);
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| match error.kind() {
            ErrorKind::InvalidData => StoreError::Content,
            _ => StoreError::Io(error),
        })?;
    if text.starts_with(BOM) {
        text.drain(..BOM.len());
    }
    snapshot(path, text, &stats)
}

/// The text of the legacy notebook file at `path`, when it is one a new notepad can start with.
fn legacy_text(path: &Path) -> Option<String> {
    let stats = fs::metadata(path).ok()?;
    if !stats.is_file() || stats.len() > MAX_TEXT_BYTES as u64 {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    let text = text.strip_prefix(BOM).unwrap_or(&text);
    (!text.trim().is_empty() && !text.contains('\0')).then(|| text.to_owned())
}

/// Write `text` to a synced temporary file beside `path`.
fn staged(path: &Path, text: &str) -> Result<tempfile::NamedTempFile, StoreError> {
    let directory = path.parent().ok_or(StoreError::Identity)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

fn create(path: &Path, legacy: Option<&Path>) -> Result<(), StoreError> {
    fs::create_dir_all(path.parent().ok_or(StoreError::Identity)?)?;
    let text = legacy.and_then(legacy_text).unwrap_or_default();
    match staged(path, &text)?.persist_noclobber(path) {
        Ok(_) => Ok(()),
        // Another writer created the file first; theirs is the notepad.
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.error.into()),
    }
}

impl NotepadStore for FileNotepadStore {
    fn open(
        &self,
        workspace_id: &str,
        legacy: Option<&Path>,
    ) -> Result<Notepad, NotepadStoreError> {
        let path = self.directory(workspace_id)?.join(FILE_NAME);
        match read(&path) {
            Err(StoreError::Io(error)) if error.kind() == ErrorKind::NotFound => {
                create(&path, legacy)?;
                Ok(read(&path)?)
            }
            outcome => Ok(outcome?),
        }
    }

    fn save(
        &self,
        workspace_id: &str,
        read_before: &Notepad,
        text: &str,
    ) -> Result<Notepad, NotepadStoreError> {
        let path = self.directory(workspace_id)?.join(FILE_NAME);
        let temporary = staged(&path, text)?;
        // Checked as late as possible: the file editor replaces this file too.
        match read(&path) {
            Ok(current) if current.revision() == read_before.revision() => {}
            Ok(_) => return Err(NotepadStoreError::Changed),
            Err(StoreError::Io(error)) if error.kind() == ErrorKind::NotFound => {
                return Err(NotepadStoreError::Changed);
            }
            Err(error) => return Err(error.into()),
        }
        let file = temporary
            .persist(&path)
            .map_err(|error| StoreError::Io(error.error))?;
        let stats = file.metadata().map_err(StoreError::Io)?;
        Ok(snapshot(&path, text.to_owned(), &stats)?)
    }

    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError> {
        match fs::remove_dir_all(self.directory(workspace_id)?) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(StoreError::Io(error).into()),
            _ => Ok(()),
        }
    }

    fn owner(&self, directory: &Path) -> Option<String> {
        if directory.parent() != Some(&self.root) {
            return None;
        }
        decode(directory.file_name()?.to_str()?)
    }
}

#[cfg(test)]
mod tests;
