use serde_json::{Value, json};
use uuid::Uuid;

use super::text;
use crate::ports::agent_session::AgentSessionError;

/// One standard ACP tool permission request with exact native option identities.
#[derive(Debug)]
pub(in crate::local) struct Pending {
    pub(in crate::local) rpc_id: Value,
    pub(in crate::local) request: Value,
    options: Vec<Value>,
}

/// Project `message` as a tool approval for `provider`; fail on invalid or oversized options.
pub(in crate::local) fn capture(
    provider: &str,
    message: &Value,
) -> Result<Pending, AgentSessionError> {
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
    let title = text(&params["toolCall"], "title")?;
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
    if params.to_string().len() > 65536 {
        return Err(AgentSessionError::Failed);
    }
    Ok(Pending {
        rpc_id,
        options: options.clone(),
        request: json!({"id":Uuid::new_v4().to_string(),"provider":provider,"name":title,
            "kind":"tool","title":title,"input":params["toolCall"]["rawInput"],"actions":actions,
            "detail":{"type":"unknown","input":params["toolCall"]["rawInput"],"output":null},
            "metadata":{"toolCallId":params["toolCall"]["toolCallId"]}}),
    })
}

/// Encode an explicit `response` to `pending`; reject absent or mismatched native choices.
pub(in crate::local) fn resolve(
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
