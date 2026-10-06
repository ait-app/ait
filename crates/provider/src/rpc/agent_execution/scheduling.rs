use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{ErrorCode, ExecutionState};
use crate::service::agent_manager::ownership::Owner;

impl ExecutionState {
    /// Clone shared services while allocating no native session or per-Agent thread.
    pub(crate) fn fork(&self, owner: Option<Owner>) -> Self {
        Self {
            manager: self.manager.fork(owner),
            owners: self.owners.clone(),
            message_observers: BTreeMap::new(),
            directory: self.directory.clone(),
            registry: self.registry.clone(),
            workspaces: self.workspaces.clone(),
            projects: self.projects.clone(),
            import_directory: self.import_directory.clone(),
            workspace_automation: self.workspace_automation.clone(),
        }
    }

    /// Retain completion text before a poll can finish and auto-archive its native writer.
    pub(crate) fn observe_messages(&mut self) {
        for id in self.manager.owned_agents() {
            if let Some(observer) = self.manager.observe_last_message(&id) {
                self.message_observers.insert(id, observer);
            }
        }
    }

    /// Publish only state that has passed the manager's durable commit boundary.
    /// # Errors
    /// Returns registry, timeline or shared observation failures.
    pub(crate) fn publish(&mut self) -> Result<(), ErrorCode> {
        self.observe_messages();
        for id in self.manager.owned_agents() {
            let mut result = match self.wait_result(&id) {
                Ok(value) => value,
                Err(ErrorCode::AgentNotFound) => json!({"status":"error","final":null,
                    "error":"Agent not found","lastMessage":null}),
                Err(error) => return Err(error),
            };
            result["busy"] = json!(self.owners.retiring(&id));
            result["live"] = json!(self.manager.live_snapshot(&id).is_some());
            result["completionText"] = json!(
                self.message_observers
                    .get(&id)
                    .and_then(|observer| observer.borrow().clone())
            );
            self.owners.publish(&id, result)?;
        }
        Ok(())
    }

    /// Compute a completion observation without polling unrelated sessions.
    /// # Errors
    /// Returns missing identity, registry or queued-input storage errors.
    pub(crate) fn wait_result(&self, id: &str) -> Result<Value, ErrorCode> {
        let snapshot = self.snapshot(id)?;
        let status = if !self.manager.interruption_pending(id)
            && (snapshot["attentionReason"] == "permission"
                || snapshot["pendingPermissions"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty()))
        {
            "permission"
        } else if self.manager.active_turn(id).is_some() || self.manager.has_pending_input(id)? {
            "running"
        } else if snapshot["status"] == "error" || snapshot["status"] == "running" {
            "error"
        } else {
            "idle"
        };
        Ok(json!({"status":status,"final":snapshot,
            "error":if status == "error" { Some("Provider execution failed") } else { None },
            "lastMessage":self.manager.last_message(id)}))
    }
}
