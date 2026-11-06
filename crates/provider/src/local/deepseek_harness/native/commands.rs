//! DSH's plugin-owned command registry; command text never becomes model input.

use serde_json::{Value, json};
use tokio::task::JoinSet;

use super::http::Api;
use crate::ports::agent_session::AgentSessionError;

pub(super) mod attachments;

/// Native command outcome plus a locally completed, private archive delivery.
#[derive(Debug)]
pub(super) struct Outcome {
    pub(super) native: Value,
    pub(super) exported: bool,
}

/// Registry and bounded in-flight work owned by one native session.
#[derive(Debug, Default)]
pub(super) struct State {
    /// Last observed effective catalog; absent only on older Hosts.
    pub(super) catalog: Option<Vec<Value>>,
    /// Admitted command requests that may still await a plugin interaction.
    pub(super) runs: JoinSet<Result<Outcome, AgentSessionError>>,
    /// At most one registry refresh at a time.
    pub(super) refresh: JoinSet<Result<Option<Vec<Value>>, AgentSessionError>>,
    /// Coalesced registry invalidations, including ones received during a refresh.
    pub(super) dirty: bool,
}

impl State {
    /// Read `agent`'s initial registry from `api`; preserve native failures.
    pub(super) async fn open(api: &Api, agent: &str) -> Result<Self, AgentSessionError> {
        Ok(Self {
            catalog: catalog(api, agent).await?,
            ..Self::default()
        })
    }

    /// Apply finished background catalog reads without interrupting native execution.
    /// Failed reads retain the last snapshot; explicit refresh still reports its own failures.
    pub(super) fn poll_refresh(&mut self) {
        while let Some(result) = self.refresh.try_join_next() {
            match result {
                Ok(Ok(catalog)) => self.catalog = catalog,
                Ok(Err(_)) | Err(_) => {
                    tracing::warn!("DSH command catalog refresh failed; previous catalog retained");
                }
            }
        }
    }
}

/// Parse DSH's exact lowercase ASCII command token, retaining the unmodified input elsewhere.
/// Returns no token for text the native registry would not parse as a command.
pub(in crate::local::deepseek_harness) fn name(line: &str) -> Option<&str> {
    let command = line.strip_prefix('/')?;
    let end = command
        .find([' ', '\t', '\n', '\r'])
        .unwrap_or(command.len());
    let name = &command[..end];
    let mut bytes = name.bytes();
    if !bytes.next()?.is_ascii_lowercase()
        || !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        return None;
    }
    Some(name)
}

/// Read the effective registry for the receiving agent without creating a new session.
/// HTTP 404 means an older Host; transport and malformed catalog errors remain failures.
pub(super) async fn catalog(
    api: &Api,
    agent: &str,
) -> Result<Option<Vec<Value>>, AgentSessionError> {
    let Some(value) = api
        .optional_call("commands/list", json!({"agentId":agent}))
        .await?
    else {
        return Ok(None);
    };
    let commands = value
        .as_array()
        .filter(|items| items.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    commands.iter().map(|command| {
        let token = crate::local::acp_config::text(command, "name")?;
        if token.len() > 256 || name(&format!("/{token}")) != Some(token) {
            return Err(AgentSessionError::Failed);
        }
        let mut entry = json!({"name":token,"description":crate::local::acp_config::text(command,"description")?,
            "argumentHint":command["input"]["hint"].as_str().unwrap_or_default(),
            "kind":"command","source":"extension",
            "acceptsAttachments":command["input"]["attachments"] == true});
        if let Some(id) = command.get("definitionId") {
            if !id.is_string() { return Err(AgentSessionError::Failed); }
            entry["definitionId"] = id.clone();
        }
        Ok(entry)
    }).collect::<Result<Vec<_>, _>>().map(Some)
}

/// Check a command's native execution acknowledgement without substituting a model prompt.
/// Returns rejected for an unmatched line, failed for an invalid acknowledgement.
pub(super) fn acknowledged(value: &Value) -> Result<(), AgentSessionError> {
    if value.is_null() {
        return Err(AgentSessionError::Rejected);
    }
    crate::local::acp_config::text(value, "commandId")?;
    if !matches!(value["result"]["kind"].as_str(), Some("success" | "error")) {
        return Err(AgentSessionError::Failed);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
