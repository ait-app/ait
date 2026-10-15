//! Bounded compaction lifecycle markers for native provider adapters.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ports::agent_session::AgentSessionError;

const MAX_ITEMS: usize = 4096;

/// Session-local native identities and the last published compaction state.
#[derive(Debug, Default)]
pub(super) struct Compactions {
    states: BTreeMap<String, &'static str>,
}

impl Compactions {
    /// Terminalize still-open operations when their native execution ends without a close event.
    pub(super) fn fail_pending(&mut self) -> Vec<Value> {
        self.states
            .iter_mut()
            .filter_map(|(id, status)| {
                if *status != "loading" {
                    return None;
                }
                *status = "failed";
                Some(json!({"type":"compaction","compactionId":id,"status":"failed"}))
            })
            .collect()
    }

    /// Return a marker for a new lifecycle state, ignoring duplicates and late starts.
    /// Invalid identities, states, triggers, or excessive distinct items return a protocol error.
    pub(super) fn marker(
        &mut self,
        id: &str,
        status: &'static str,
        trigger: Option<&str>,
    ) -> Result<Option<Value>, AgentSessionError> {
        if id.is_empty()
            || id.len() > 512
            || !matches!(status, "loading" | "completed" | "failed")
            || trigger.is_some_and(|trigger| !matches!(trigger, "auto" | "manual"))
        {
            return Err(AgentSessionError::Failed);
        }
        if let Some(previous) = self.states.get(id) {
            if *previous != "loading" || *previous == status {
                return Ok(None);
            }
        } else if self.states.len() >= MAX_ITEMS {
            return Err(AgentSessionError::Failed);
        }
        self.states.insert(id.to_owned(), status);
        let mut item = json!({"type":"compaction","compactionId":id,"status":status});
        if let Some(trigger) = trigger {
            item["trigger"] = json!(trigger);
        }
        Ok(Some(item))
    }
}

#[cfg(test)]
mod tests;
