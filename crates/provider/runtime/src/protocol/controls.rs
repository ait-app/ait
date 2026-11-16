//! Remaining Agent controls and Provider inspection requests.

use serde::Deserialize;
use serde_json::Value;

use super::agent_execution::SessionConfig;
use super::timeline::{Cursor, Direction};

/// A non-destructive native conversation rewind target.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RewindRequest {
    /// Registered Agent identifier.
    pub(crate) agent_id: String,
    /// Native user message to remove together with subsequent turns.
    pub(crate) message_id: String,
    /// Only conversation is supported by Codex; files and both are explicitly rejected.
    pub(crate) mode: String,
}

/// Discover commands for an existing Agent or an unregistered draft.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CommandsRequest {
    /// Existing Agent identifier, or a UI draft identifier.
    pub(crate) agent_id: String,
    /// Native working directory and configuration when the Agent does not exist.
    pub(crate) draft_config: Option<SessionConfig>,
}

/// Resolve a pending native approval scoped to one live Agent session.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PermissionRequest {
    /// Registered Agent identifier.
    pub(crate) agent_id: String,
    /// Permission identity from the pending request, distinct from the RPC envelope ID.
    pub(crate) request_id: String,
    /// Allow/deny decision, with optional question answers.
    pub(crate) response: Value,
}

/// Query a provider-owned descendant without registering another host Agent.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SubagentRequest {
    /// Registered root Agent identifier.
    pub(crate) parent_agent_id: String,
    /// Native descendant ID for timeline requests.
    pub(crate) subagent_id: Option<String>,
    /// Pagination direction.
    pub(crate) direction: Option<Direction>,
    /// Exclusive generation/sequence boundary.
    pub(crate) cursor: Option<Cursor>,
    /// Page size, with zero meaning the bounded full window.
    pub(crate) limit: Option<usize>,
}
