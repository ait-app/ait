//! Transport-independent notepad request decoding and projection.

use domain::workspace::notepad::{
    Author, AuthorKind, MAX_NOTES, MAX_TEXT_BYTES, Note, NotepadError, Write,
};
use model::methods::MethodSpec;
use model::rpc::{decode, encode};
use model::workspace::notepad::NotepadStoreError;
use serde_json::Value;

use crate::notepad::protocol::{
    NoteSummary, NotepadChangeResult, NotepadDeleteRequest, NotepadGetRequest, NotepadGetResult,
    NotepadLimits, NotepadListRequest, NotepadListResult, NotepadWriteRequest,
};
use crate::notepad::service::{NotepadFailure, WorkspaceNotepads};
use crate::support::error::ErrorCode;
use crate::support::time::now_timestamp;

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("workspace.notepad.list.request"),
    MethodSpec::request("workspace.notepad.get.request"),
    MethodSpec::request("workspace.notepad.write.request"),
    MethodSpec::request("workspace.notepad.delete.request"),
];

/// Characters of a note's text shown in a listing.
const PREVIEW_CHARS: usize = 160;

/// Decode and execute one notepad request.
///
/// # Errors
/// Returns `WorkspaceNotFound` for an unknown or archived Workspace, `InvalidMessage` for
/// malformed parameters, keys, text or authors, and storage failures. A refused write or delete
/// is a successful response with `accepted: false`.
pub(crate) fn execute(
    notepads: &WorkspaceNotepads,
    method: &str,
    params: Value,
) -> Result<Value, ErrorCode> {
    match method {
        "workspace.notepad.list.request" => list(notepads, &decode(params)?),
        "workspace.notepad.get.request" => get(notepads, &decode(params)?),
        "workspace.notepad.write.request" => write(notepads, decode(params)?),
        "workspace.notepad.delete.request" => delete(notepads, &decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn list(notepads: &WorkspaceNotepads, request: &NotepadListRequest) -> Result<Value, ErrorCode> {
    let notepad = notepads
        .notepad(&request.workspace_id)
        .map_err(|failure| code(&failure))?;
    let since = request.since.unwrap_or(0);
    let notes = notepad
        .notes()
        .filter(|note| note.notepad_revision > since)
        .map(summary)
        .collect();
    encode(
        NotepadListResult {
            workspace_id: &request.workspace_id,
            revision: notepad.revision(),
            total: notepad.notes().count(),
            notes,
            limits: NotepadLimits {
                notes: MAX_NOTES,
                text_bytes: MAX_TEXT_BYTES,
            },
        },
        ErrorCode::RegistryIo,
    )
}

fn summary(note: &Note) -> NoteSummary<'_> {
    let end = note
        .text
        .char_indices()
        .nth(PREVIEW_CHARS)
        .map_or(note.text.len(), |(index, _)| index);
    NoteSummary {
        key: &note.key,
        revision: note.revision,
        notepad_revision: note.notepad_revision,
        chars: note.text.chars().count(),
        preview: &note.text[..end],
        created_by: &note.created_by,
        updated_by: &note.updated_by,
        created_at: &note.created_at,
        updated_at: &note.updated_at,
    }
}

fn get(notepads: &WorkspaceNotepads, request: &NotepadGetRequest) -> Result<Value, ErrorCode> {
    let notepad = notepads
        .notepad(&request.workspace_id)
        .map_err(|failure| code(&failure))?;
    encode(
        NotepadGetResult {
            workspace_id: &request.workspace_id,
            revision: notepad.revision(),
            note: notepad.note(&request.key),
        },
        ErrorCode::RegistryIo,
    )
}

fn write(notepads: &WorkspaceNotepads, request: NotepadWriteRequest) -> Result<Value, ErrorCode> {
    let now = now_timestamp();
    let outcome = notepads.write(
        &request.workspace_id,
        Write {
            key: &request.key,
            text: &request.text,
            mode: request.mode,
            expected: request.expected,
            author: request
                .author
                .unwrap_or(Author::anonymous(AuthorKind::User)),
            now: &now,
        },
    );
    let outcome = outcome.map(|note| (Some(note.revision), note.notepad_revision));
    change(&request.workspace_id, &request.key, outcome)
}

fn delete(
    notepads: &WorkspaceNotepads,
    request: &NotepadDeleteRequest,
) -> Result<Value, ErrorCode> {
    let outcome = notepads
        .delete(&request.workspace_id, &request.key, request.expected)
        .map(|notepad_revision| (None, notepad_revision));
    change(&request.workspace_id, &request.key, outcome)
}

/// Encode an accepted change, or a refusal the caller can act on, as a response.
fn change(
    workspace_id: &str,
    key: &str,
    outcome: Result<(Option<u64>, u64), NotepadFailure>,
) -> Result<Value, ErrorCode> {
    let mut result = NotepadChangeResult {
        accepted: false,
        workspace_id,
        key,
        revision: None,
        notepad_revision: None,
        updated_by: None,
        error: None,
    };
    let refused = match outcome {
        Ok((revision, notepad_revision)) => {
            result.accepted = true;
            result.revision = revision;
            result.notepad_revision = Some(notepad_revision);
            return encode(result, ErrorCode::RegistryIo);
        }
        Err(NotepadFailure::Refused(
            refused @ (NotepadError::Conflict(_)
            | NotepadError::Exists(_)
            | NotepadError::Missing
            | NotepadError::Full
            | NotepadError::TooLarge),
        )) => refused,
        Err(failure) => return Err(code(&failure)),
    };
    result.error = Some(refused.to_string());
    if let NotepadError::Conflict(note) | NotepadError::Exists(note) = &refused {
        result.revision = Some(note.revision);
        result.updated_by = Some(&note.updated_by);
    }
    encode(result, ErrorCode::RegistryIo)
}

/// The stable error code of a failure that is not a refusal a caller can retry.
fn code(failure: &NotepadFailure) -> ErrorCode {
    match failure {
        NotepadFailure::Workspace => ErrorCode::WorkspaceNotFound,
        NotepadFailure::Storage(NotepadStoreError::Full) => ErrorCode::ResourceExhausted,
        NotepadFailure::Storage(NotepadStoreError::Io) => ErrorCode::RegistryIo,
        NotepadFailure::Refused(_) => ErrorCode::InvalidMessage,
    }
}

#[cfg(test)]
mod tests;
