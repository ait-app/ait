//! Ports through which Workspace Bot tickets follow pull requests (ADR-287): pull requests are
//! read with the host's forge CLI in the Bot's checkout, on a blocking thread and one at a time;
//! a ticket's Agents come from the Agent registry, and inputs reach them through the same
//! execution path as `send_agent_prompt`, only while the Agent is idle, so they never
//! interrupt, steer into or queue behind a turn.

use std::sync::{Arc, Mutex};

use bot::follow::{Delivered, PullReadError, PullSource, Workers};
use domain::TicketId;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentRuntimeStatus, PersistedAgentRuntimeRecord};
use domain::bot::BOT_TICKET_LABEL;
use domain::forge::PullSnapshot;
use filesystem::forge::service::Forge;
use model::ErrorCode;
use model::workspace::registry::WorkspaceRegistry;
use provider::service::agent_execution::AgentExecution;
use serde_json::{Value, json};

/// Newest timeline entries of each Agent searched for what it wrote on a pull request.
const TIMELINE_ENTRIES: u64 = 200;

/// Reads pull requests with the forge CLI in a Workspace's checkout.
pub(super) struct ForgePulls {
    /// One forge for every read, so reads never run concurrently.
    pub(super) forge: Arc<Mutex<Forge>>,
    pub(super) workspaces: Arc<dyn WorkspaceRegistry>,
}

#[async_trait::async_trait]
impl PullSource for ForgePulls {
    async fn read(&self, workspace: &str, number: u64) -> Result<PullSnapshot, PullReadError> {
        let forge = self.forge.clone();
        let workspaces = self.workspaces.clone();
        let workspace = workspace.to_owned();
        tokio::task::spawn_blocking(move || {
            let cwd = checkout(workspaces.as_ref(), &workspace)?;
            let forge = forge
                .lock()
                .map_err(|_| PullReadError::Failed("forge unavailable".to_owned()))?;
            forge.pull_request(&cwd, number).map_err(|error| {
                if error.is_not_found() {
                    PullReadError::NotFound
                } else if error.is_unavailable() {
                    PullReadError::Unavailable(error.to_string())
                } else {
                    PullReadError::Failed(error.to_string())
                }
            })
        })
        .await
        .map_err(|_| PullReadError::Failed("pull request read was cancelled".to_owned()))?
    }

    async fn can_write(&self, workspace: &str, login: &str) -> Result<bool, PullReadError> {
        let forge = self.forge.clone();
        let workspaces = self.workspaces.clone();
        let workspace = workspace.to_owned();
        let login = login.to_owned();
        tokio::task::spawn_blocking(move || {
            let cwd = checkout(workspaces.as_ref(), &workspace)?;
            let forge = forge
                .lock()
                .map_err(|_| PullReadError::Failed("forge unavailable".to_owned()))?;
            forge
                .can_write(&cwd, &login)
                .map_err(|error| PullReadError::Failed(error.to_string()))
        })
        .await
        .map_err(|_| PullReadError::Failed("write access lookup was cancelled".to_owned()))?
    }
}

/// The checkout directory of Workspace `workspace`.
///
/// # Errors
/// Returns a failure when the registry cannot be read and `Unavailable` when the Workspace is
/// gone.
fn checkout(workspaces: &dyn WorkspaceRegistry, workspace: &str) -> Result<String, PullReadError> {
    Ok(workspaces
        .get(workspace)
        .map_err(|error| PullReadError::Failed(error.to_string()))?
        .ok_or_else(|| PullReadError::Unavailable("the Workspace is gone".to_owned()))?
        .cwd)
}

/// Reaches a ticket's Agents through the Agent registry and execution.
pub(super) struct AgentWorkers {
    pub(super) agents: Arc<dyn AgentRuntimeRegistry>,
    pub(super) execution: AgentExecution,
}

#[async_trait::async_trait]
impl Workers for AgentWorkers {
    async fn live(&self, ticket: TicketId) -> Result<Vec<String>, ErrorCode> {
        let registry = self.agents.clone();
        let records = tokio::task::spawn_blocking(move || registry.snapshot())
            .await
            .map_err(|_| ErrorCode::AgentIo)?
            .map_err(|_| ErrorCode::AgentIo)?;
        Ok(live(records.iter().map(AsRef::as_ref), ticket))
    }

    async fn inputs(&self, agents: &[String]) -> Vec<String> {
        let mut inputs = Vec::new();
        for agent in agents {
            let page = self
                .execution
                .execute(
                    "agent.timeline.get.request",
                    json!({"agentId":agent,"direction":"tail","limit":TIMELINE_ENTRIES}),
                )
                .await;
            if let Ok(page) = page {
                tool_inputs(&page, &mut inputs);
            }
        }
        inputs
    }

    async fn deliver(
        &self,
        agent: &str,
        message_id: &str,
        text: &str,
    ) -> Result<Delivered, ErrorCode> {
        let registry = self.agents.clone();
        let id = agent.to_owned();
        let record = tokio::task::spawn_blocking(move || registry.get(&id))
            .await
            .map_err(|_| ErrorCode::AgentIo)?
            .map_err(|_| ErrorCode::AgentIo)?
            .filter(|record| record.archived_at.as_deref().is_none_or(str::is_empty))
            .ok_or(ErrorCode::AgentNotFound)?;
        if !idle(record.last_status) {
            return Ok(Delivered::Busy);
        }
        // `steer` only matters if a turn started since the check: the input then joins that
        // turn rather than interrupting it.
        let sent = self
            .execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":agent,"text":text,"messageId":message_id,
                    "activeTurnBehavior":"steer"}),
            )
            .await?;
        if sent["accepted"] == true {
            Ok(Delivered::Started)
        } else {
            Err(ErrorCode::AgentIo)
        }
    }
}

/// Whether an Agent in `status` has no turn running, so an input starts a new one.
const fn idle(status: AgentRuntimeStatus) -> bool {
    match status {
        AgentRuntimeStatus::Idle | AgentRuntimeStatus::Closed | AgentRuntimeStatus::Error => true,
        AgentRuntimeStatus::Initializing | AgentRuntimeStatus::Running => false,
    }
}

/// The unarchived Agents among `records` whose ticket label names `ticket`.
fn live<'a>(
    records: impl Iterator<Item = &'a PersistedAgentRuntimeRecord>,
    ticket: TicketId,
) -> Vec<String> {
    let ticket = ticket.to_string();
    records
        .filter(|record| {
            record.labels.get(BOT_TICKET_LABEL) == Some(&ticket)
                && record.archived_at.as_deref().is_none_or(str::is_empty)
        })
        .map(|record| record.id.clone())
        .collect()
}

/// Push what the tool calls of timeline `page` sent onto `inputs`: the strings of each
/// `tool_call` entry's detail except its results (`output`, `result`, `log`, and the `content`
/// a read or search returned). User and assistant messages, delivered inputs and tool output
/// are left out. The display timeline keeps tool arguments up to its per-field preview bound
/// (32 KiB for Claude), well above a forge comment.
fn tool_inputs(page: &Value, inputs: &mut Vec<String>) {
    let entries = page["entries"].as_array().map_or(&[][..], Vec::as_slice);
    for item in entries.iter().map(|entry| &entry["item"]) {
        if item["type"] != "tool_call" {
            continue;
        }
        let Some(detail) = item["detail"].as_object() else {
            continue;
        };
        let returned = matches!(
            detail.get("type").and_then(Value::as_str),
            Some("read" | "search")
        );
        for (key, value) in detail {
            let result = matches!(key.as_str(), "output" | "result" | "log")
                || (returned && key == "content");
            if !result {
                strings(value, inputs);
            }
        }
    }
}

/// Push every string `value` holds, at any depth, onto `texts`.
fn strings(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::String(text) => texts.push(text.clone()),
        Value::Array(values) => values.iter().for_each(|value| strings(value, texts)),
        Value::Object(fields) => fields.values().for_each(|value| strings(value, texts)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests;
