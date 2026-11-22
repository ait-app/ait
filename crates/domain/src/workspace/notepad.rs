//! Workspace notepad: the one text every Agent and user of a Workspace shares (ADR-296).
//!
//! Pure values and transitions; storage and transport live in the outer crates.

use serde::{Deserialize, Serialize};

/// Most bytes a notepad's text holds, including appended attribution lines.
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
    /// The revision the caller read; required to replace a notepad that was ever written.
    pub expected: Option<u64>,
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

/// One Workspace's shared text. A Workspace that was never written to has the default notepad:
/// empty text at revision 0.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notepad {
    text: String,
    revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_by: Option<Author>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
}

impl Notepad {
    /// The shared text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Counts every write; 0 until the first one.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Who wrote last, once anyone has.
    #[must_use]
    pub const fn updated_by(&self) -> Option<&Author> {
        self.updated_by.as_ref()
    }

    /// RFC 3339 time of the last write, once there was one.
    #[must_use]
    pub fn updated_at(&self) -> Option<&str> {
        self.updated_at.as_deref()
    }

    /// Apply `write`: replace the text when `expected` is the current revision (it may be
    /// omitted only while the notepad was never written), or append a block under a line naming
    /// its author and time.
    ///
    /// # Errors
    ///
    /// Returns [`NotepadError`] for invalid text or author, a missing or stale expected
    /// revision, or an oversized result; the notepad is then unchanged.
    pub fn write(&mut self, write: Write<'_>) -> Result<(), NotepadError> {
        let appending = write.mode == WriteMode::Append;
        if write.text.contains('\0')
            || (appending && write.text.is_empty())
            || !write.author.is_valid()
        {
            return Err(NotepadError::Invalid);
        }
        let text = if appending {
            let separator = if self.text.is_empty() { "" } else { "\n\n" };
            format!(
                "{}{separator}[{} · {}]\n{}",
                self.text,
                write.author.label(),
                write.now,
                write.text
            )
        } else if write.expected.unwrap_or(0) == self.revision {
            write.text.to_owned()
        } else {
            return Err(NotepadError::Conflict);
        };
        if text.len() > MAX_TEXT_BYTES {
            return Err(NotepadError::TooLarge);
        }
        self.text = text;
        self.revision += 1;
        self.updated_by = Some(write.author);
        self.updated_at = Some(write.now.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests;
