//! Bounded, read-only BTW context from committed display messages, excluding live progress.

use provider_core::protocol::side_questions::{SideContextEntry, encoded_size};

use super::{ErrorCode, Timeline, io};
use crate::storage::side::redact_text;

impl Timeline {
    /// Read the newest committed readable context within its serialized byte budget.
    ///
    /// Does not register a timeline, read native history, or include `progress` rows.
    /// User input envelopes use their display text before redaction and budget accounting.
    /// Tool cards contribute only their displayed name, never inputs or permission payloads.
    /// # Errors
    /// Returns storage or malformed committed-entry errors.
    pub(crate) fn side_context(
        &self,
        agent: &str,
        budget: usize,
    ) -> Result<(Vec<SideContextEntry>, bool), ErrorCode> {
        let database = self.database.lock().map_err(io)?;
        let mut statement = database
            .prepare(
                "SELECT json_extract(entry,'$.item.type'),
             CASE WHEN json_extract(entry,'$.item.type')='tool_call'
               THEN json_extract(entry,'$.item.name')
               ELSE json_extract(entry,'$.item.text') END
             FROM entries WHERE agent=?1 AND json_extract(entry,'$.item.type')
               IN ('user_message','assistant_message','tool_call') ORDER BY seq DESC",
            )
            .map_err(io)?;
        let mut rows = statement.query([agent]).map_err(io)?;
        let mut entries = Vec::new();
        let mut truncated: bool = database
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM progress WHERE agent=?1)",
                [agent],
                |row| row.get(0),
            )
            .map_err(io)?;
        let mut remaining = budget;
        while let Some(row) = rows.next().map_err(io)? {
            let kind: String = row.get(0).map_err(io)?;
            let Some(text) = row.get::<_, Option<String>>(1).map_err(io)? else {
                continue;
            };
            let role = match kind.as_str() {
                "user_message" => "user",
                "assistant_message" => "assistant",
                "tool_call" => "tool",
                _ => continue,
            };
            let envelope = if role == "user" {
                domain::input_envelope::parse(&text)
            } else {
                None
            };
            let display_text = envelope
                .as_ref()
                .map_or(text.as_str(), |envelope| envelope.text.as_ref());
            if display_text.is_empty() {
                continue;
            }
            let mut entry = SideContextEntry {
                role: role.to_owned(),
                text: redact_text(display_text),
            };
            let separator = usize::from(!entries.is_empty());
            let mut size = encoded_size(&entry).map_err(io)? + separator;
            let mut clipped = false;
            if size > remaining {
                let overhead = size - encoded_size(&entry.text).map_err(io)? + 2;
                if remaining <= overhead {
                    truncated = true;
                    break;
                }
                let available = remaining - overhead;
                let mut used = 0;
                let mut end = 0;
                for (offset, character) in entry.text.char_indices() {
                    let width = match character {
                        '"' | '\\' | '\n' | '\t' => 2,
                        _ => character.len_utf8(),
                    };
                    if used + width > available {
                        break;
                    }
                    used += width;
                    end = offset + character.len_utf8();
                }
                entry.text.truncate(end);
                size = overhead + used;
                clipped = true;
                truncated = true;
            }
            remaining -= size;
            entries.push(entry);
            if clipped {
                break;
            }
        }
        entries.reverse();
        Ok((entries, truncated))
    }
}

#[cfg(test)]
mod tests;
