//! Transport-independent notepad request decoding and projection.

use std::path::Path;

use domain::workspace::notepad::{Author, AuthorKind, NotepadError, Write};
use model::methods::MethodSpec;
use model::rpc::{decode, encode};
use model::workspace::notepad::NotepadStoreError;
use serde_json::Value;

use crate::notepad::protocol::{
    NotepadGetRequest, NotepadGetResult, NotepadWriteRequest, NotepadWriteResult,
};
use crate::notepad::service::{NotepadFailure, WorkspaceNotepads};
use crate::support::error::ErrorCode;
use crate::support::time::now_timestamp;

/// Client methods implemented by this component; consumed by capability discovery.
pub(crate) const METHODS: &[MethodSpec] = &[
    MethodSpec::request("workspace.notepad.get.request"),
    MethodSpec::request("workspace.notepad.write.request"),
];

/// Decode and execute one notepad request.
///
/// # Errors
/// Returns `WorkspaceNotFound` for an unknown or archived Workspace, `InvalidMessage` for
/// malformed parameters, text or authors, and storage failures. A write refused for a stale
/// revision or for size is a successful response with `accepted: false`.
pub(crate) fn execute(
    notepads: &WorkspaceNotepads,
    method: &str,
    params: Value,
) -> Result<Value, ErrorCode> {
    match method {
        "workspace.notepad.get.request" => get(notepads, &decode(params)?),
        "workspace.notepad.write.request" => write(notepads, decode(params)?),
        _ => Err(ErrorCode::MethodNotFound),
    }
}

fn get(notepads: &WorkspaceNotepads, request: &NotepadGetRequest) -> Result<Value, ErrorCode> {
    let notepad = notepads
        .notepad(&request.workspace_id)
        .map_err(|failure| code(&failure))?;
    let path = Path::new(notepad.path());
    encode(
        NotepadGetResult {
            workspace_id: &request.workspace_id,
            path: notepad.path(),
            directory: &path.parent().unwrap_or(path).to_string_lossy(),
            revision: notepad.revision(),
            text: notepad.text(),
            updated_at: notepad.updated_at(),
        },
        ErrorCode::RegistryIo,
    )
}

fn write(notepads: &WorkspaceNotepads, request: NotepadWriteRequest) -> Result<Value, ErrorCode> {
    let now = now_timestamp();
    let outcome = notepads.write(
        &request.workspace_id,
        &Write {
            text: &request.text,
            mode: request.mode,
            expected: request.expected.as_deref(),
            author: request
                .author
                .unwrap_or(Author::anonymous(AuthorKind::User)),
            now: &now,
        },
    );
    let (notepad, error) = match outcome {
        Ok(notepad) => (notepad, None),
        Err(NotepadFailure::Refused {
            reason: reason @ (NotepadError::Conflict | NotepadError::TooLarge),
            current,
        }) => (*current, Some(reason.to_string())),
        Err(failure) => return Err(code(&failure)),
    };
    encode(
        NotepadWriteResult {
            accepted: error.is_none(),
            workspace_id: &request.workspace_id,
            path: notepad.path(),
            revision: notepad.revision(),
            updated_at: notepad.updated_at(),
            error,
        },
        ErrorCode::RegistryIo,
    )
}

/// The stable error code of a failure that is not a refusal a caller can retry.
fn code(failure: &NotepadFailure) -> ErrorCode {
    match failure {
        NotepadFailure::Workspace => ErrorCode::WorkspaceNotFound,
        NotepadFailure::Storage(NotepadStoreError::Full) => ErrorCode::ResourceExhausted,
        NotepadFailure::Storage(NotepadStoreError::Io | NotepadStoreError::Changed) => {
            ErrorCode::RegistryIo
        }
        NotepadFailure::Refused { .. } => ErrorCode::InvalidMessage,
    }
}

#[cfg(test)]
mod tests;
