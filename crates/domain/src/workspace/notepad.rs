//! Workspace notepad: keyed notes the Agents of one Workspace share (ADR-296).
//!
//! Pure values and transitions; storage and transport live in the outer crates.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Most notes one Workspace notepad holds.
pub const MAX_NOTES: usize = 64;
/// Most bytes one note's text holds, including appended attribution lines.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;
/// Most bytes a note key holds.
pub const MAX_KEY_BYTES: usize = 64;
/// Most characters of an author's identifier or title kept with a note.
const MAX_AUTHOR_CHARS: usize = 128;

/// What kind of caller wrote a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorKind {
    /// A public Agent.
    Agent,
    /// The Workspace Bot's own session.
    Bot,
    /// A person using a client.
    User,
    /// An MCP caller that named no Agent.
    External,
}

/// Who wrote a note. With a host credential the Agent identity is claimed, not verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Author {
    /// Caller kind.
    pub kind: AuthorKind,
    /// The writing Agent, for [`AuthorKind::Agent`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// The Agent's title when it wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Author {
    /// An author of `kind` without an Agent identity.
    #[must_use]
    pub const fn anonymous(kind: AuthorKind) -> Self {
        Self {
            kind,
            agent_id: None,
            title: None,
        }
    }

    /// Whether the identity fields are bounded, free of control characters and consistent with
    /// the kind: only an Agent author names an Agent.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let bounded = |value: &Option<String>| {
            value.as_deref().is_none_or(|value| {
                !value.is_empty()
                    && value.chars().count() <= MAX_AUTHOR_CHARS
                    && !value.chars().any(char::is_control)
            })
        };
        let named = self.agent_id.is_some();
        bounded(&self.agent_id) && bounded(&self.title) && named == (self.kind == AuthorKind::Agent)
    }

    /// Short text naming the author in an appended block.
    fn label(&self) -> &str {
        match self.kind {
            AuthorKind::Agent => self
                .title
                .as_deref()
                .or(self.agent_id.as_deref())
                .unwrap_or("Agent"),
            AuthorKind::Bot => "Workspace Bot",
            AuthorKind::User => "User",
            AuthorKind::External => "External caller",
        }
    }
}

/// One keyed note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Note {
    /// Caller-chosen key, unique in its notepad.
    pub key: String,
    /// Note text.
    pub text: String,
    /// Starts at 1 and grows by one with every write of this note.
    pub revision: u64,
    /// The notepad revision that last changed this note.
    pub notepad_revision: u64,
    /// Who created the note.
    pub created_by: Author,
    /// Who wrote it last.
    pub updated_by: Author,
    /// RFC 3339 creation time.
    pub created_at: String,
    /// RFC 3339 time of the last write.
    pub updated_at: String,
}

/// How a write combines with an existing note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    /// Create the note or replace its text, guarded by the expected revision.
    #[default]
    Replace,
    /// Add an attributed block at the end, creating the note when absent.
    Append,
}

/// One write to a notepad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write<'a> {
    /// Target note key.
    pub key: &'a str,
    /// New text, or the block to append.
    pub text: &'a str,
    /// Replace or append.
    pub mode: WriteMode,
    /// The note revision the caller read; required to replace an existing note.
    pub expected: Option<u64>,
    /// Who writes.
    pub author: Author,
    /// RFC 3339 time of the write.
    pub now: &'a str,
}

/// Why a notepad change was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotepadError {
    /// The key is empty, too long or uses characters outside `[a-z0-9-]`.
    #[error(
        "note key must be 1-64 characters of a-z, 0-9 and '-', starting with a letter or digit"
    )]
    InvalidKey,
    /// The text is empty or contains NUL, or the author is malformed.
    #[error("note text must not be empty or contain NUL")]
    InvalidText,
    /// The expected revision does not match the note; carries its current state.
    #[error("the note changed since it was read; read it again and retry with its revision")]
    Conflict(Box<Note>),
    /// A replace of an existing note carried no expected revision; carries its current state.
    #[error("the note already exists; pass its revision as expected, or use mode append")]
    Exists(Box<Note>),
    /// The note does not exist.
    #[error("the note does not exist")]
    Missing,
    /// The notepad already holds [`MAX_NOTES`] notes.
    #[error("the notepad is full; merge or delete notes that are no longer needed")]
    Full,
    /// The note would exceed [`MAX_TEXT_BYTES`].
    #[error("the note would exceed 16 KiB; shorten it or start another note")]
    TooLarge,
}

/// A committed change, for the store to persist with the new notepad revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteChange {
    /// The note was created or rewritten.
    Put(Note),
    /// The note with this key was removed.
    Remove(String),
}

/// One Workspace's notes and the revision counting every change to them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notepad {
    revision: u64,
    notes: BTreeMap<String, Note>,
}

/// Whether `key` is 1 to [`MAX_KEY_BYTES`] characters of `[a-z0-9-]` starting with a letter or
/// digit.
#[must_use]
pub fn valid_key(key: &str) -> bool {
    let allowed = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    key.len() <= MAX_KEY_BYTES
        && key.bytes().next().is_some_and(allowed)
        && key.bytes().all(|byte| allowed(byte) || byte == b'-')
}

impl Notepad {
    /// Rebuild a notepad from stored `notes` and its `revision`.
    #[must_use]
    pub fn restore(revision: u64, notes: impl IntoIterator<Item = Note>) -> Self {
        Self {
            revision,
            notes: notes
                .into_iter()
                .map(|note| (note.key.clone(), note))
                .collect(),
        }
    }

    /// The revision counting every change to this notepad.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Notes in key order.
    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.values()
    }

    /// The note stored under `key`.
    #[must_use]
    pub fn note(&self, key: &str) -> Option<&Note> {
        self.notes.get(key)
    }

    /// Apply `write` and return the change to persist.
    ///
    /// # Errors
    ///
    /// Returns [`NotepadError`] for an invalid key, text or author, a missing or mismatched
    /// expected revision, a full notepad or an oversized note; the notepad is then unchanged.
    pub fn write(&mut self, write: Write<'_>) -> Result<NoteChange, NotepadError> {
        if !valid_key(write.key) {
            return Err(NotepadError::InvalidKey);
        }
        if write.text.is_empty() || write.text.contains('\0') || !write.author.is_valid() {
            return Err(NotepadError::InvalidText);
        }
        let existing = self.notes.get(write.key);
        let text = match (write.mode, existing) {
            (WriteMode::Append, existing) => appended(existing, &write),
            (WriteMode::Replace, None) if write.expected.is_some() => {
                return Err(NotepadError::Missing);
            }
            (WriteMode::Replace, Some(note)) if write.expected.is_none() => {
                return Err(NotepadError::Exists(Box::new(note.clone())));
            }
            (WriteMode::Replace, Some(note)) if write.expected != Some(note.revision) => {
                return Err(NotepadError::Conflict(Box::new(note.clone())));
            }
            (WriteMode::Replace, _) => write.text.to_owned(),
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err(NotepadError::TooLarge);
        }
        if existing.is_none() && self.notes.len() >= MAX_NOTES {
            return Err(NotepadError::Full);
        }
        self.revision += 1;
        let note = Note {
            key: write.key.to_owned(),
            text,
            revision: existing.map_or(1, |note| note.revision + 1),
            notepad_revision: self.revision,
            created_by: existing
                .map_or_else(|| write.author.clone(), |note| note.created_by.clone()),
            created_at: existing
                .map_or_else(|| write.now.to_owned(), |note| note.created_at.clone()),
            updated_by: write.author,
            updated_at: write.now.to_owned(),
        };
        self.notes.insert(note.key.clone(), note.clone());
        Ok(NoteChange::Put(note))
    }

    /// Remove the note under `key` when its revision equals `expected`.
    ///
    /// # Errors
    ///
    /// Returns [`NotepadError::Missing`] for an absent note and [`NotepadError::Conflict`] for
    /// another revision; the notepad is then unchanged.
    pub fn delete(&mut self, key: &str, expected: u64) -> Result<NoteChange, NotepadError> {
        let note = self.notes.get(key).ok_or(NotepadError::Missing)?;
        if note.revision != expected {
            return Err(NotepadError::Conflict(Box::new(note.clone())));
        }
        self.notes.remove(key);
        self.revision += 1;
        Ok(NoteChange::Remove(key.to_owned()))
    }
}

/// The text of `existing` followed by the written block under a line naming its author and time.
fn appended(existing: Option<&Note>, write: &Write<'_>) -> String {
    let previous = existing.map_or("", |note| note.text.as_str());
    let separator = if previous.is_empty() { "" } else { "\n\n" };
    format!(
        "{previous}{separator}[{} · {}]\n{}",
        write.author.label(),
        write.now,
        write.text
    )
}

#[cfg(test)]
mod tests;
