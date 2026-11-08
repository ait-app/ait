//! Default-deny request policy for automation sessions.
//!
//! The center may create Agents in the directories the owner allowed, drive the
//! Agents it created, see whether their timelines grow (for its idle timeout, never
//! their entries), and nothing else: no terminals, files, settings, shares, Bots,
//! permission decisions or daemon management. This limits RPCs, not what an
//! Agent's own processes do on the Host.

use std::path::{Path, PathBuf};

use model::automation::{ORIGIN, ORIGIN_LABEL, Settings};
use model::{ErrorCode, Request};
use serde_json::{Map, Value, json};

use crate::Shared;

/// Top-level `agent.create.request` fields an automation session may send.
const CREATE_FIELDS: [&str; 3] = ["config", "labels", "idempotencyKey"];

/// Native configuration an automation session may choose.
const CONFIG_FIELDS: [&str; 7] = [
    "provider",
    "model",
    "cwd",
    "title",
    "modeId",
    "thinkingOptionId",
    "mcpServers",
];

/// `agent.timeline.get.request` fields the center sends; the answer ignores their values.
const ACTIVITY_FIELDS: [&str; 3] = ["agentId", "direction", "limit"];

/// `agent.message.send.request` fields; text only, with a retry identity.
const SEND_FIELDS: [&str; 4] = ["agentId", "text", "messageId", "activeTurnBehavior"];

/// The only MCP server the center may inject: its own reply tools.
const TOOL_SERVER: &str = "ait_hub";

/// Label namespaces the center may not write: guest attribution and Bot scope.
const RESERVED_LABEL_PREFIXES: [&str; 2] = ["ait.guest.", domain::bot::BOT_LABEL_PREFIX];

/// Admit `request` from an automation session, rewriting its parameters in place.
/// `Some` answers it here instead of forwarding it.
///
/// # Errors
/// Returns `Forbidden` for methods and arguments outside the automation policy, and
/// `InvalidMessage` for malformed arguments.
pub(crate) async fn request(
    state: &Shared,
    request: &mut Request,
) -> Result<Option<Value>, ErrorCode> {
    let settings = state.automation.get();
    if !settings.enabled {
        return Err(ErrorCode::Forbidden);
    }
    match request.method.as_str() {
        "agent.create.request" => create(&settings, &mut request.params).await?,
        "agent.message.send.request" => {
            only(&request.params, &SEND_FIELDS)?;
            if !request.params["messageId"].is_string() {
                return Err(ErrorCode::InvalidMessage);
            }
            owned(state, &mut request.params).await?;
        }
        "agent.finish.wait.request" | "agent.cancel.request" | "agent.get.request" => {
            owned(state, &mut request.params).await?;
        }
        "agent.timeline.get.request" => {
            only(&request.params, &ACTIVITY_FIELDS)?;
            owned(state, &mut request.params).await?;
            return activity(state, &request.params["agentId"]).await.map(Some);
        }
        _ => return Err(ErrorCode::Forbidden),
    }
    Ok(None)
}

/// The size of an Agent's timeline window, without any entry.
async fn activity(state: &Shared, agent: &Value) -> Result<Value, ErrorCode> {
    let execution = state
        .provider
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::Forbidden)?;
    let page = execution
        .execute(
            "agent.timeline.get.request",
            json!({"agentId":agent,"direction":"tail","limit":1}),
        )
        .await
        .map_err(ErrorCode::from)?;
    Ok(json!({"agentId":agent,"epoch":page["epoch"],"window":page["window"]}))
}

/// Reject any field outside `allowed`.
fn only(params: &Value, allowed: &[&str]) -> Result<(), ErrorCode> {
    let object = params.as_object().ok_or(ErrorCode::InvalidMessage)?;
    if object.keys().all(|key| allowed.contains(&key.as_str())) {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// Confine a new Agent to an allowed directory and provider, and mark it as automation's.
async fn create(settings: &Settings, params: &mut Value) -> Result<(), ErrorCode> {
    only(params, &CREATE_FIELDS)?;
    if !params["idempotencyKey"].is_string() {
        return Err(ErrorCode::InvalidMessage);
    }
    let config = &params["config"];
    only(config, &CONFIG_FIELDS)?;
    let provider = config["provider"]
        .as_str()
        .ok_or(ErrorCode::InvalidMessage)?;
    if !settings.admits_provider(provider) {
        return Err(ErrorCode::Forbidden);
    }
    tools(&config["mcpServers"])?;
    let cwd = config["cwd"].as_str().ok_or(ErrorCode::InvalidMessage)?;
    let cwd = inside_roots(settings, Path::new(cwd)).await?;
    params["config"]["cwd"] = json!(cwd.to_string_lossy());
    let labels = params
        .as_object_mut()
        .ok_or(ErrorCode::InvalidMessage)?
        .entry("labels")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or(ErrorCode::InvalidMessage)?;
    if labels.iter().any(|(key, value)| {
        !value.is_string()
            || RESERVED_LABEL_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
    }) {
        return Err(ErrorCode::Forbidden);
    }
    labels.insert(ORIGIN_LABEL.to_owned(), json!(ORIGIN));
    Ok(())
}

/// Only the center's own reply tools, over HTTP(S).
fn tools(servers: &Value) -> Result<(), ErrorCode> {
    let Some(servers) = servers.as_object() else {
        return if servers.is_null() {
            Ok(())
        } else {
            Err(ErrorCode::InvalidMessage)
        };
    };
    let allowed = servers.iter().all(|(name, server)| {
        name == TOOL_SERVER
            && server["type"] == "http"
            && server["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("https://") || url.starts_with("http://"))
    });
    if allowed {
        Ok(())
    } else {
        Err(ErrorCode::Forbidden)
    }
}

/// The canonical `directory` when it lies within an allowed root.
async fn inside_roots(settings: &Settings, directory: &Path) -> Result<PathBuf, ErrorCode> {
    let directory = tokio::fs::canonicalize(directory)
        .await
        .map_err(|_| ErrorCode::Forbidden)?;
    for root in &settings.roots {
        // A root that does not exist admits nothing.
        if let Ok(root) = tokio::fs::canonicalize(&root.path).await
            && directory.starts_with(&root)
        {
            return Ok(directory);
        }
    }
    Err(ErrorCode::Forbidden)
}

/// Require the Agent named by `agentId` to be one automation created, and pin its full ID.
async fn owned(state: &Shared, params: &mut Value) -> Result<(), ErrorCode> {
    let identifier = params["agentId"]
        .as_str()
        .ok_or(ErrorCode::InvalidMessage)?
        .to_owned();
    let execution = state
        .provider
        .agent_execution
        .as_ref()
        .ok_or(ErrorCode::Forbidden)?;
    let value = execution
        .execute("agent.get.request", json!({ "agentId": identifier }))
        .await
        .map_err(|code| match ErrorCode::from(code) {
            // The center starts a new conversation for a missing Agent.
            ErrorCode::AgentNotFound => ErrorCode::AgentNotFound,
            _ => ErrorCode::Forbidden,
        })?;
    let agent = &value["agent"];
    if agent["labels"][ORIGIN_LABEL] != ORIGIN {
        return Err(ErrorCode::Forbidden);
    }
    params["agentId"] = agent["id"].clone();
    Ok(())
}

#[cfg(test)]
mod tests;
