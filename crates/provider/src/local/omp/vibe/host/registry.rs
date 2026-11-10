use std::sync::Arc;

use super::{MAX_WORKERS, Registry, Shared};
use crate::ports::agent_session::AgentSessionError;

impl Registry {
    /// Reserve room for a new identity by retiring the oldest unused terminal record.
    /// Returns Busy if every retained identity is active, starting or pinned by an operation.
    pub(super) fn make_room(&mut self, restoring: bool) -> Result<(), AgentSessionError> {
        if restoring {
            return Ok(());
        }
        while self.records.len() + self.starting.len() >= MAX_WORKERS {
            let oldest = self
                .records
                .iter()
                .filter(|(id, record)| {
                    matches!(
                        record.child.descriptor["status"].as_str(),
                        Some("completed" | "failed" | "canceled")
                    ) && !self.starting.contains_key(*id)
                        && !self.users.contains_key(*id)
                })
                .min_by_key(|(_, record)| {
                    record.child.descriptor["updatedAt"]
                        .as_str()
                        .unwrap_or_default()
                })
                .map(|(id, _)| id.clone())
                .ok_or(AgentSessionError::Busy)?;
            self.records.remove(&oldest);
            self.slots.remove(&oldest);
        }
        Ok(())
    }
}

/// Pin records while sending, killing or waiting so eviction cannot invalidate an operation.
pub(super) struct Usage {
    shared: Arc<Shared>,
    ids: Vec<String>,
}

impl Usage {
    /// Retain each requested identity until this guard drops; reject unknown records atomically.
    pub(super) fn acquire(
        shared: Arc<Shared>,
        ids: Vec<String>,
    ) -> Result<Self, AgentSessionError> {
        {
            let mut registry = shared
                .registry
                .lock()
                .map_err(|_| AgentSessionError::Failed)?;
            if ids.iter().any(|id| !registry.records.contains_key(id)) {
                return Err(AgentSessionError::Rejected);
            }
            for id in &ids {
                *registry.users.entry(id.clone()).or_default() += 1;
            }
        }
        Ok(Self { shared, ids })
    }
}

impl Drop for Usage {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.shared.registry.lock() {
            for id in &self.ids {
                if let Some(count) = registry.users.get_mut(id) {
                    *count -= 1;
                    if *count == 0 {
                        registry.users.remove(id);
                    }
                }
            }
        }
    }
}
