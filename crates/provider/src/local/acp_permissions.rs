//! Shared ACP tool-permission callbacks for stdio ACP adapters.

use serde_json::{Value, json};
use uuid::Uuid;

use super::acp_config::text;
use crate::ports::agent_session::AgentSessionError;

/// Correlated native callback retained until exactly one validated response is flushed.
#[derive(Debug)]
pub(crate) struct Pending {
    /// Native JSON-RPC identity answered by the host response.
    pub(crate) rpc_id: Value,
    /// Host-facing permission or question request.
    pub(crate) request: Value,
    /// Native choices accepted for a tool permission; empty for a question.
    pub(crate) options: Vec<Value>,
    /// Retained elicitation schema, or none for a tool permission.
    pub(crate) schema: Option<Value>,
    /// Schema properties whose answers must stay unique.
    pub(crate) unique_fields: Vec<String>,
}

/// Project native permission choices; malformed identities and option lists fail closed.
///
/// # Arguments
///
/// * `message` - Native `session/request_permission` callback.
/// * `provider` - Stable provider identity recorded on the host request.
///
/// # Returns
/// The retained callback, ready to answer with exactly one validated choice.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing identity, an empty or oversized
/// option list, duplicate option identities, unknown option kinds or oversized retained fields.
pub(crate) fn capture(message: &Value, provider: &str) -> Result<Pending, AgentSessionError> {
    let params = &message["params"];
    let rpc_id = message
        .get("id")
        .filter(|id| id.is_string() || id.is_number())
        .ok_or(AgentSessionError::Failed)?
        .clone();
    let options = params["options"]
        .as_array()
        .filter(|options| !options.is_empty() && options.len() <= 128)
        .ok_or(AgentSessionError::Failed)?;
    text(&params["toolCall"], "toolCallId")?;
    // Native shell approvals use the full command as their display title, including newlines.
    // The identifier validator would reject ordinary scripts and long commands as protocol errors.
    let title = params["toolCall"]["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty() && !title.contains('\0'))
        .ok_or(AgentSessionError::Failed)?;
    let mut ids = std::collections::BTreeSet::new();
    let actions = options
        .iter()
        .map(|option| {
            let id = text(option, "optionId")?;
            if !ids.insert(id) {
                return Err(AgentSessionError::Failed);
            }
            let behavior = match option["kind"].as_str() {
                Some("allow_once" | "allow_always") => "allow",
                Some("reject_once" | "reject_always") => "deny",
                _ => return Err(AgentSessionError::Failed),
            };
            Ok(json!({"id":id,"label":text(option, "name")?,"behavior":behavior}))
        })
        .collect::<Result<Vec<_>, AgentSessionError>>()?;
    // OpenCode includes full-file diff previews in toolCall.content. Those unretained fields
    // are already bounded by the transport frame limit. Bound the fields we retain before
    // cloning them into the UI's repeated title/input fields.
    if serde_json::to_vec(&(
        &params["toolCall"]["toolCallId"],
        title,
        &params["toolCall"]["rawInput"],
        options,
    ))
    .map_err(|_| AgentSessionError::Failed)?
    .len()
        > 65536
    {
        return Err(AgentSessionError::Failed);
    }
    Ok(Pending {
        rpc_id,
        options: options.clone(),
        schema: None,
        unique_fields: Vec::new(),
        request: json!({"id":Uuid::new_v4().to_string(),"provider":provider,"name":title,
            "kind":"tool","title":title,"input":params["toolCall"]["rawInput"],"actions":actions,
            "detail":{"type":"unknown","input":params["toolCall"]["rawInput"],"output":null},
            "metadata":{"toolCallId":params["toolCall"]["toolCallId"]}}),
    })
}

/// Validate a host tool answer and build its exact native callback reply.
///
/// # Arguments
///
/// * `pending` - Retained callback for this host request.
/// * `response` - Host answer carrying `behavior` and an optional selected action.
///
/// # Returns
/// The native JSON-RPC reply selecting exactly one advertised option.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for unknown fields, a selected action that the
/// host did not advertise, or a behavior the callback does not accept.
pub(crate) fn resolve_tool(
    pending: &Pending,
    response: &Value,
) -> Result<Value, AgentSessionError> {
    let fields = response.as_object().ok_or(AgentSessionError::Rejected)?;
    if fields.keys().any(|key| {
        !matches!(
            key.as_str(),
            "behavior" | "selectedActionId" | "interrupt" | "message"
        )
    }) || response
        .get("interrupt")
        .is_some_and(|value| !value.is_boolean())
        || response.get("message").is_some_and(|value| {
            response["behavior"] != "deny"
                || !value
                    .as_str()
                    .is_some_and(|text| text.len() <= 4096 && !text.contains('\0'))
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    let kind = match response["behavior"].as_str() {
        Some("allow") => "allow_once",
        Some("deny") => "reject_once",
        _ => return Err(AgentSessionError::Rejected),
    };
    let selected = if let Some(id) = response.get("selectedActionId") {
        pending
            .options
            .iter()
            .find(|option| option["optionId"] == *id)
            .filter(|option| {
                option["kind"].as_str().is_some_and(|selected| {
                    selected.starts_with(if kind == "allow_once" {
                        "allow_"
                    } else {
                        "reject_"
                    })
                })
            })
            .ok_or(AgentSessionError::Rejected)?
    } else {
        pending
            .options
            .iter()
            .find(|option| option["kind"] == kind)
            .ok_or(AgentSessionError::Rejected)?
    };
    Ok(json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":{
        "outcome":{"outcome":"selected","optionId":selected["optionId"]}}}))
}

#[cfg(test)]
mod tests;
