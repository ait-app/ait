//! End what revoked guests started: their terminals and running Agent turns.
//!
//! Agent history is kept; only active turns and pending permission requests are cancelled.

use std::sync::{Arc, Weak};

use provider::service::agent_execution::AgentExecution;
use provider::service::agent_mcp::{GUEST_SHARE_LABEL, GUEST_USER_LABEL};
use serde_json::{Value, json};

use super::guests::{Grant, Revoked};

/// Services that own guest-created resources. Held weakly: the provider's workspace
/// cleanup hook holds the share book, so strong references would form a cycle that
/// keeps application storage, and with it the data-directory lease, alive forever.
#[derive(Debug, Clone)]
pub(super) struct Owners {
    terminal: Weak<terminal::dispatch::State>,
    provider: Weak<provider::dispatch::State>,
}

impl Owners {
    pub(super) fn new(
        terminal: &Arc<terminal::dispatch::State>,
        provider: &Arc<provider::dispatch::State>,
    ) -> Self {
        Self {
            terminal: Arc::downgrade(terminal),
            provider: Arc::downgrade(provider),
        }
    }

    /// Run [`Owners::run`] on the provider's tracked tasks so shutdown waits for it.
    pub(super) fn spawn(self, revoked: Vec<Revoked>, handle: &tokio::runtime::Handle) {
        if let Some(provider) = self.provider.upgrade() {
            provider.tasks.spawn_on(self.run(revoked), handle);
        }
    }

    /// Kill the revoked grants' terminals and cancel their Agents' turns.
    /// Does nothing once the services shut down.
    pub(super) async fn run(self, revoked: Vec<Revoked>) {
        let (Some(terminal), Some(provider)) = (self.terminal.upgrade(), self.provider.upgrade())
        else {
            return;
        };
        let terminals: Vec<String> = revoked
            .iter()
            .flat_map(|revoked| revoked.terminals.iter().cloned())
            .collect();
        if !terminals.is_empty()
            && terminal::dispatch::revoke(&terminal, terminals)
                .await
                .is_err()
        {
            tracing::error!("revoked guest terminals could not be closed");
        }
        if let Some(execution) = &provider.agent_execution {
            for revoked in &revoked {
                cancel_agents(execution, revoked.grant).await;
            }
        }
    }
}

async fn cancel_agents(execution: &AgentExecution, grant: Grant) {
    let filter = json!({"filter":{"labels":{
        GUEST_SHARE_LABEL: grant.share_id.to_string(),
        GUEST_USER_LABEL: grant.user_id.to_string(),
    }}});
    let listed = match execution.execute("agent.list.request", filter).await {
        Ok(listed) => listed,
        Err(code) => {
            tracing::error!(?code, "revoked guest Agents could not be listed");
            return;
        }
    };
    for id in agent_ids(&listed) {
        if let Err(code) = execution
            .execute("agent.cancel.request", json!({"agentId": id}))
            .await
        {
            tracing::warn!(?code, "revoked guest Agent turn could not be cancelled");
        }
    }
}

/// Agent identities of an `agent.list.request` result.
pub(super) fn agent_ids(listed: &Value) -> Vec<&str> {
    listed["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry["agent"]["id"].as_str())
                .collect()
        })
        .unwrap_or_default()
}
