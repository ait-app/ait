//! Small durable native control results absent from a provider's transcript file.

use serde_json::Value;

use provider_core::ports::agent_session::AgentSessionError;
use provider_core::protocol::timeline::NativeItem;

/// Bounded native timeline notes persisted with the session's saved state.
#[derive(Debug, Clone, Default)]
pub struct Notes(Vec<NativeItem>);

impl Notes {
    /// Resume from the value previously returned by [`Self::saved`].
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Failed`] for a malformed value and
    /// [`AgentSessionError::Rejected`] when it exceeds the bounds.
    pub fn restore(value: Option<&Value>) -> Result<Self, AgentSessionError> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let entries: Vec<NativeItem> =
            serde_json::from_value(value.clone()).map_err(|_| AgentSessionError::Failed)?;
        let mut notes = Self::default();
        for entry in entries {
            notes.push(entry)?;
        }
        Ok(notes)
    }

    /// Record `entry`, ignoring an identical repeat of its key.
    ///
    /// # Errors
    ///
    /// Returns [`AgentSessionError::Failed`] when the key already holds a different item and
    /// [`AgentSessionError::Rejected`] when the notes would exceed 128 entries or 64 KiB.
    pub fn push(&mut self, entry: NativeItem) -> Result<(), AgentSessionError> {
        if let Some(previous) = self.0.iter().find(|previous| previous.key == entry.key) {
            return if previous.item == entry.item {
                Ok(())
            } else {
                Err(AgentSessionError::Failed)
            };
        }
        if self.0.len() >= 128
            || serde_json::to_vec(&self.0)
                .map_err(|_| AgentSessionError::Failed)?
                .len()
                + serde_json::to_vec(&entry)
                    .map_err(|_| AgentSessionError::Failed)?
                    .len()
                > 64 * 1024
        {
            return Err(AgentSessionError::Rejected);
        }
        self.0.push(entry);
        Ok(())
    }

    /// The persisted form of the notes, or `None` when there are none.
    #[must_use]
    pub fn saved(&self) -> Option<Value> {
        (!self.0.is_empty()).then(|| serde_json::json!(self.0))
    }

    /// Insert notes missing from `entries` in timestamp order.
    pub fn history(&self, entries: &mut Vec<NativeItem>) {
        for entry in &self.0 {
            if !entries.iter().any(|existing| existing.key == entry.key) {
                let timestamp = chrono::DateTime::parse_from_rfc3339(&entry.timestamp).ok();
                let position = timestamp
                    .and_then(|timestamp| {
                        entries.iter().position(|existing| {
                            chrono::DateTime::parse_from_rfc3339(&existing.timestamp)
                                .is_ok_and(|existing| existing > timestamp)
                        })
                    })
                    .unwrap_or(entries.len());
                entries.insert(position, entry.clone());
            }
        }
    }

    /// Keep only notes whose turn still appears in `entries`.
    pub fn retain(&mut self, entries: &[NativeItem]) {
        self.0.retain(|note| {
            note.turn_id.as_ref().is_some_and(|turn| {
                entries
                    .iter()
                    .any(|entry| entry.turn_id.as_ref() == Some(turn))
            })
        });
    }
}

#[cfg(test)]
mod tests;
