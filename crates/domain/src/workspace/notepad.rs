//! Workspace notepad: the one text file every Agent and user of a Workspace shares (ADR-296,
//! ADR-297).
//!
//! Pure values and transitions; storage and transport live in the outer crates.

use serde::{Deserialize, Serialize};

/// Most bytes a notepad write may leave in the file, including appended attribution lines.
pub const MAX_TEXT_BYTES: usize = 256 * 1024;
/// Most characters of an author's identifier or title kept with a notepad.
const MAX_AUTHOR_CHARS: usize = 128;

/// What kind of caller wrote to a notepad.
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

/// Who wrote to a notepad. With a host credential the Agent identity is claimed, not verified.
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

/// How a write combines with the notepad's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    /// Replace the whole text, guarded by the expected revision.
    #[default]
    Replace,
    /// Add an attributed block at the end.
    Append,
}

/// One write to a notepad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write<'a> {
    /// The new text, or the block to append.
    pub text: &'a str,
    /// Replace or append.
    pub mode: WriteMode,
    /// The revision the caller read; required to replace a notepad that holds text.
    pub expected: Option<&'a str>,
    /// Who writes.
    pub author: Author,
    /// RFC 3339 time of the write.
    pub now: &'a str,
}

/// Why a write was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NotepadError {
    /// The text contains NUL, an appended block is empty, or the author is malformed.
    #[error("notepad text must not contain NUL and an appended block must not be empty")]
    Invalid,
    /// A replace did not carry the notepad's current revision.
    #[error(
        "the notepad changed since it was read; read it again and pass its revision as \
         expected, or add to it with mode append"
    )]
    Conflict,
    /// The text would exceed [`MAX_TEXT_BYTES`].
    #[error("the notepad would exceed 256 KiB; shorten it before adding more")]
    TooLarge,
}

/// One Workspace's notepad as its file held it when it was read. The file belongs to the host,
/// outside the Workspace's checkout, and people edit it like any other file, so every value is
/// a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notepad {
    path: String,
    text: String,
    revision: String,
    updated_at: String,
}

impl Notepad {
    /// The notepad file at `path` holding `text`, identified by `revision` and last modified at
    /// the RFC 3339 time `updated_at`.
    #[must_use]
    pub const fn new(path: String, text: String, revision: String, updated_at: String) -> Self {
        Self {
            path,
            text,
            revision,
            updated_at,
        }
    }

    /// Absolute path of the notepad file on the host.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The shared text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Opaque token that differs whenever the text does.
    #[must_use]
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// RFC 3339 time the file was last modified.
    #[must_use]
    pub fn updated_at(&self) -> &str {
        &self.updated_at
    }

    /// The text `write` leaves in this notepad: its own text when `expected` is the current
    /// revision (it may be omitted only while the notepad is empty), or the current text
    /// followed by a block under a line naming its author and time.
    ///
    /// # Errors
    ///
    /// Returns [`NotepadError`] for invalid text or author, a missing or stale expected
    /// revision, or an oversized result.
    pub fn written(&self, write: &Write<'_>) -> Result<String, NotepadError> {
        let block = write.text.trim_end_matches('\n');
        let appending = write.mode == WriteMode::Append;
        if write.text.contains('\0') || (appending && block.is_empty()) || !write.author.is_valid()
        {
            return Err(NotepadError::Invalid);
        }
        let text = if appending {
            // A blank line separates the block from text that does not already end with one.
            let separator = match self.text.as_bytes() {
                [] | [.., b'\n', b'\n'] => "",
                [.., b'\n'] => "\n",
                _ => "\n\n",
            };
            format!(
                "{}{separator}[{} · {}]\n{block}\n",
                self.text,
                write.author.label(),
                write.now
            )
        } else if write
            .expected
            .map_or(self.text.is_empty(), |expected| expected == self.revision)
        {
            write.text.to_owned()
        } else {
            return Err(NotepadError::Conflict);
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err(NotepadError::TooLarge);
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests;
