//! Display titles derived from existing user input, without creating provider turns.

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use model::ErrorCode;

use super::AgentManager;
use provider_core::ports::native_history::SessionHistory;
use provider_core::protocol::timeline::NativeItem;

/// Return whether `record` is public and has no nonblank display title.
pub(super) fn missing(record: &PersistedAgentRuntimeRecord) -> bool {
    !record.internal
        && record.observation.is_none()
        && record
            .title
            .as_deref()
            .is_none_or(|title| title.trim().is_empty())
}

/// Normalize `text` into at most 200 UTF-16 units, returning `None` for empty input.
pub(super) fn from_prompt(text: &str) -> Option<String> {
    let mut title = String::new();
    let mut units = 0;
    let mut space = false;
    for character in text.chars() {
        if character.is_whitespace() || character.is_control() {
            space = !title.is_empty();
            continue;
        }
        let length = character.len_utf16() + usize::from(space);
        if units + length > 200 {
            break;
        }
        if space {
            title.push(' ');
        }
        title.push(character);
        units += length;
        space = false;
    }
    (!title.is_empty()).then_some(title)
}

/// Return the first nonempty user title from ordered `entries`, ignoring other item types.
pub(super) fn from_entries(entries: &[NativeItem]) -> Option<String> {
    entries
        .iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .filter_map(|entry| entry.item["text"].as_str())
        .find_map(from_prompt)
}

/// Prefer `history`'s native title and preview, then fall back to its user messages.
pub(super) fn from_history(history: &SessionHistory) -> Option<String> {
    history
        .descriptor
        .title
        .as_deref()
        .and_then(from_prompt)
        .or_else(|| {
            history
                .descriptor
                .first_prompt_preview
                .as_deref()
                .and_then(from_prompt)
        })
        .or_else(|| from_entries(&history.entries))
}

impl AgentManager {
    /// Fill untitled public records from the local projection before serving directory requests.
    /// Returns storage errors without launching a provider or rewriting history.
    pub(crate) fn recover_titles(&self) -> Result<(), ErrorCode> {
        let Some(timeline) = &self.timeline else {
            return Ok(());
        };
        for record in self.registry.list().map_err(|_| ErrorCode::AgentIo)? {
            if missing(&record) {
                let title = timeline
                    .first_user_text(&record.id)?
                    .as_deref()
                    .and_then(from_prompt);
                self.fill_missing_title(&record.id, title)?;
            }
        }
        Ok(())
    }

    /// Save `title` for `id` only while its current record is public and untitled.
    /// An absent candidate is a no-op; missing records and storage failures return errors.
    pub(super) fn fill_missing_title(
        &self,
        id: &str,
        title: Option<String>,
    ) -> Result<(), ErrorCode> {
        let Some(title) = title else {
            return Ok(());
        };
        self.registry
            .update(id, &|current| {
                let mut next = current.clone();
                if missing(current) {
                    next.title = Some(title.clone());
                    next.title_origin = Some(domain::agent_runtime::TitleOrigin::Prompt);
                }
                next
            })
            .map_err(|_| ErrorCode::AgentIo)?
            .ok_or(ErrorCode::AgentNotFound)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
