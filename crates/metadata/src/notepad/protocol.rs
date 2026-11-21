//! Notepad request and response shapes.

use domain::workspace::notepad::{Author, Note, WriteMode};
use serde::{Deserialize, Serialize};

/// `workspace.notepad.list.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadListRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
    /// Return only notes changed after this notepad revision.
    #[serde(default)]
    pub(crate) since: Option<u64>,
}

/// `workspace.notepad.get.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadGetRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
    /// Note key.
    pub(crate) key: String,
}

/// `workspace.notepad.write.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadWriteRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
    /// Note key.
    pub(crate) key: String,
    /// New text, or the block to append.
    pub(crate) text: String,
    /// Replace (the default) or append.
    #[serde(default)]
    pub(crate) mode: WriteMode,
    /// The note revision read before replacing an existing note.
    #[serde(default)]
    pub(crate) expected: Option<u64>,
    /// Who writes; a person using a client when absent.
    #[serde(default)]
    pub(crate) author: Option<Author>,
}

/// `workspace.notepad.delete.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadDeleteRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
    /// Note key.
    pub(crate) key: String,
    /// The note revision read before deleting.
    pub(crate) expected: u64,
}

/// A note without its full text.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NoteSummary<'a> {
    /// Note key.
    pub(crate) key: &'a str,
    /// Note revision.
    pub(crate) revision: u64,
    /// The notepad revision that last changed the note.
    pub(crate) notepad_revision: u64,
    /// Length of the text in characters.
    pub(crate) chars: usize,
    /// The start of the text.
    pub(crate) preview: &'a str,
    /// Who created the note.
    pub(crate) created_by: &'a Author,
    /// Who wrote it last.
    pub(crate) updated_by: &'a Author,
    /// RFC 3339 creation time.
    pub(crate) created_at: &'a str,
    /// RFC 3339 time of the last write.
    pub(crate) updated_at: &'a str,
}

/// Bounds a caller needs to stay within.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadLimits {
    /// Most notes in one notepad.
    pub(crate) notes: usize,
    /// Most bytes of one note's text.
    pub(crate) text_bytes: usize,
}

/// `workspace.notepad.list.request` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadListResult<'a> {
    /// Target Workspace.
    pub(crate) workspace_id: &'a str,
    /// Current notepad revision; pass it as `since` to read later changes.
    pub(crate) revision: u64,
    /// Number of notes in the notepad, including those `since` left out.
    pub(crate) total: usize,
    /// Notes in key order.
    pub(crate) notes: Vec<NoteSummary<'a>>,
    /// Notepad bounds.
    pub(crate) limits: NotepadLimits,
}

/// `workspace.notepad.get.request` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadGetResult<'a> {
    /// Target Workspace.
    pub(crate) workspace_id: &'a str,
    /// Current notepad revision.
    pub(crate) revision: u64,
    /// The note, or none when no note has this key.
    pub(crate) note: Option<&'a Note>,
}

/// Result of a write or delete: either accepted, or refused with what the caller needs to retry.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadChangeResult<'a> {
    /// Whether the change was stored.
    pub(crate) accepted: bool,
    /// Target Workspace.
    pub(crate) workspace_id: &'a str,
    /// Note key.
    pub(crate) key: &'a str,
    /// The note's revision: after an accepted write, or the current one after a refusal that
    /// found the note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<u64>,
    /// The notepad revision after an accepted change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notepad_revision: Option<u64>,
    /// Who last wrote the note a refusal found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_by: Option<&'a Author>,
    /// Why the change was refused and how to proceed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}
