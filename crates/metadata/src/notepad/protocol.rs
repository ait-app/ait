//! Notepad request and response shapes.

use domain::workspace::notepad::{Author, WriteMode};
use serde::{Deserialize, Serialize};

/// `workspace.notepad.get.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadGetRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
}

/// `workspace.notepad.write.request` parameters.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotepadWriteRequest {
    /// Target Workspace.
    pub(crate) workspace_id: String,
    /// The new text, or the block to append.
    pub(crate) text: String,
    /// Replace (the default) or append.
    #[serde(default)]
    pub(crate) mode: WriteMode,
    /// The revision read before replacing.
    #[serde(default)]
    pub(crate) expected: Option<u64>,
    /// Who writes; a person using a client when absent.
    #[serde(default)]
    pub(crate) author: Option<Author>,
}

/// `workspace.notepad.get.request` result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadGetResult<'a> {
    /// Target Workspace.
    pub(crate) workspace_id: &'a str,
    /// Counts every write; 0 for a notepad never written to.
    pub(crate) revision: u64,
    /// The shared text.
    pub(crate) text: &'a str,
    /// Who wrote last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_by: Option<&'a Author>,
    /// RFC 3339 time of the last write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<&'a str>,
}

/// `workspace.notepad.write.request` result: accepted, or refused with what a retry needs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotepadWriteResult<'a> {
    /// Whether the write was stored.
    pub(crate) accepted: bool,
    /// Target Workspace.
    pub(crate) workspace_id: &'a str,
    /// The notepad's revision: the new one after an accepted write, the current one otherwise.
    pub(crate) revision: u64,
    /// Who wrote last.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_by: Option<&'a Author>,
    /// RFC 3339 time of the last write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) updated_at: Option<&'a str>,
    /// Why the write was refused and how to proceed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}
