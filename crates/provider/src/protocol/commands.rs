//! Read-only native editor completion, independent of prompt admission and execution.

use serde::{Deserialize, Serialize};

use crate::ports::agent_session::AgentSessionError;

const MAX_TEXT: usize = 65_536;
const MAX_ITEMS: usize = 256;
const MAX_ITEMS_BYTES: usize = 896 * 1024;

/// Full editor state; all offsets use UTF-16 code units, as in ACP and JavaScript editors.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletionInput {
    /// Current editor contents, including text following the cursor.
    pub(crate) text: String,
    /// Caret offset, which must not bisect a Unicode surrogate pair.
    pub(crate) cursor: usize,
}

impl CompletionInput {
    /// Validate size and caret boundaries before native I/O.
    /// Returns rejected for malformed or oversized editor state.
    pub(crate) fn validate(&self) -> Result<(), AgentSessionError> {
        if self.text.len() > MAX_TEXT || self.text.contains('\0') {
            return Err(AgentSessionError::Rejected);
        }
        byte_offset(&self.text, self.cursor)
            .map(|_| ())
            .ok_or(AgentSessionError::Rejected)
    }
}

/// Exclusive replacement range in the original input, measured in UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletionRange {
    /// Inclusive start offset.
    pub(crate) start: usize,
    /// Exclusive end offset.
    pub(crate) end: usize,
}

/// One original native candidate with an exact edit and native ordering.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletionItem {
    /// Request-local candidate identity.
    pub(crate) id: String,
    /// Display label; never used as an execution command.
    pub(crate) label: String,
    /// Exact replacement produced by the native completer, including quoting and spacing.
    pub(crate) insert_text: String,
    /// Original text range replaced on acceptance.
    pub(crate) replace_range: CompletionRange,
    /// Native description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    /// Native usage or secondary detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
    /// Native inline hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) hint: Option<String>,
    /// Optional future-compatible candidate category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<String>,
    /// Native caret offset in the resulting editor contents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cursor: Option<usize>,
    /// Continue querying after acceptance, for namespaces and nested arguments.
    #[serde(default)]
    pub(crate) trigger_completion: bool,
}

/// A supported empty result is authoritative; unsupported permits catalog fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompletionResult {
    /// The native owner implements editor completion for this session.
    pub(crate) supported: bool,
    /// Candidates in native order; clients must not rerank them.
    pub(crate) items: Vec<CompletionItem>,
}

impl CompletionResult {
    /// Validate native edits against `input` without exposing malformed payloads.
    /// Returns failed for inconsistent support, excessive output or invalid UTF-16 ranges.
    pub(crate) fn validate(&self, input: &CompletionInput) -> Result<(), AgentSessionError> {
        input.validate()?;
        if self.items.len() > MAX_ITEMS || (!self.supported && !self.items.is_empty()) {
            return Err(AgentSessionError::Failed);
        }
        let mut bytes = 0_usize;
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            bytes = bytes.saturating_add(
                serde_json::to_vec(item)
                    .map_err(|_| AgentSessionError::Failed)?
                    .len(),
            );
            if bytes > MAX_ITEMS_BYTES
                || item.id.is_empty()
                || item.id.len() > 256
                || item.label.len() > 8192
                || item.insert_text.len() > MAX_TEXT
                || item.insert_text.contains('\0')
                || !ids.insert(&item.id)
                || item.replace_range.start > item.replace_range.end
            {
                return Err(AgentSessionError::Failed);
            }
            let start = byte_offset(&input.text, item.replace_range.start)
                .ok_or(AgentSessionError::Failed)?;
            let end = byte_offset(&input.text, item.replace_range.end)
                .ok_or(AgentSessionError::Failed)?;
            if input.text.len() - (end - start) + item.insert_text.len() > MAX_TEXT {
                return Err(AgentSessionError::Failed);
            }
            if let Some(cursor) = item.cursor {
                let edited = format!(
                    "{}{}{}",
                    &input.text[..start],
                    item.insert_text,
                    &input.text[end..]
                );
                if byte_offset(&edited, cursor).is_none() {
                    return Err(AgentSessionError::Failed);
                }
            }
        }
        Ok(())
    }
}

fn byte_offset(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += character.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

#[cfg(test)]
mod tests;
