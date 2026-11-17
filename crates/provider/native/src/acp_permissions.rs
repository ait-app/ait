//! Shared ACP tool-permission and form callbacks for stdio ACP adapters.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use uuid::Uuid;

use super::acp_config::text;
use super::elicitation;
use super::tool_detail::{MARKER, input_preview, truncate};
use provider_core::ports::agent_session::AgentSessionError;

/// Display budget for a native permission title, which may be a complete shell script.
const TITLE_BYTES: usize = 16 * 1024;

/// Correlated native callback retained until exactly one validated response is flushed.
#[derive(Debug)]
pub struct Pending {
    /// Native JSON-RPC identity answered by the host response.
    pub rpc_id: Value,
    /// Host-facing permission or question request.
    pub request: Value,
    /// Native choices accepted for a tool permission; empty for a question.
    pub options: Vec<Value>,
    /// Retained elicitation schema, or none for a tool permission.
    pub schema: Option<Value>,
    /// Schema properties whose answers must stay unique.
    pub unique_fields: Vec<String>,
}

/// Project native permission choices; malformed identities and option lists fail closed.
///
/// # Arguments
///
/// * `message` - Native `session/request_permission` callback.
/// * `provider` - Stable provider identity recorded on the host request.
///
/// # Returns
/// The retained callback, ready to answer with exactly one validated choice. Its host request
/// shows bounded previews of the native title and raw input.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing identity, an empty or oversized
/// option list, duplicate option identities or unknown option kinds.
pub fn capture(message: &Value, provider: &str) -> Result<Pending, AgentSessionError> {
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
    let mut ids = BTreeSet::new();
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
    // OpenCode includes full-file diff previews in toolCall.content, and native titles and raw
    // input can carry complete commands or file contents. The native reply selects only an
    // option, so the UI's repeated title/input fields receive bounded previews.
    let title = truncate(title, TITLE_BYTES, MARKER);
    let input = input_preview(&params["toolCall"]["rawInput"]);
    Ok(Pending {
        rpc_id,
        options: options.clone(),
        schema: None,
        unique_fields: Vec::new(),
        request: json!({"id":Uuid::new_v4().to_string(),"provider":provider,"name":title,
            "kind":"tool","title":title,"input":input,"actions":actions,
            "detail":{"type":"unknown","input":input,"output":null},
            "metadata":{"toolCallId":params["toolCall"]["toolCallId"]}}),
    })
}

/// Build the native reply that withdraws an unanswered permission or form without a choice.
///
/// ACP requires every pending `session/request_permission` to be answered with the
/// `cancelled` outcome once its prompt turn is cancelled.
///
/// # Arguments
///
/// * `pending` - Retained callback that the host has not answered.
///
/// # Returns
/// The native JSON-RPC reply: a `cancel` action for a form, otherwise a `cancelled` outcome.
#[must_use]
pub fn cancelled(pending: &Pending) -> Value {
    let result = if pending.schema.is_some() {
        json!({"action":"cancel"})
    } else {
        json!({"outcome":{"outcome":"cancelled"}})
    };
    json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":result})
}

/// Retain a native permission or form callback for the active prompt of one session.
///
/// # Arguments
///
/// * `message` - Native `session/request_permission` or `elicitation/create` callback.
/// * `session` - Native session identity the callback must address.
/// * `active` - Whether a prompt is currently running in that session.
/// * `pending` - Callbacks awaiting a host answer, keyed by host request identity.
/// * `project` - Projection of the admitted callback, such as [`capture`] or [`form`].
///
/// # Returns
/// The host-facing request to publish.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` outside an active prompt, for another session,
/// beyond 128 pending callbacks or for a repeated native identity; otherwise returns the
/// error with which `project` rejects the callback.
pub fn admit(
    message: &Value,
    session: &str,
    active: bool,
    pending: &mut BTreeMap<String, Pending>,
    project: impl FnOnce(&Value) -> Result<Pending, AgentSessionError>,
) -> Result<Value, AgentSessionError> {
    if !active
        || message["params"]["sessionId"] != session
        || pending.len() >= 128
        || pending
            .values()
            .any(|retained| retained.rpc_id == message["id"])
    {
        return Err(AgentSessionError::Failed);
    }
    let captured = project(message)?;
    let request = captured.request.clone();
    pending.insert(text(&request, "id")?.to_owned(), captured);
    Ok(request)
}

/// Validate a host tool answer and build its exact native callback reply.
///
/// Returns the native JSON-RPC reply selecting exactly one advertised option, or
/// `AgentSessionError::Rejected` for unknown fields, a selected action that the host did
/// not advertise, or a behavior the callback does not accept.
fn resolve_tool(pending: &Pending, response: &Value) -> Result<Value, AgentSessionError> {
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

/// Validate a host answer and build its exact native callback reply.
///
/// # Arguments
///
/// * `pending` - Retained permission or form callback for this host request.
/// * `response` - Host answer carrying `behavior` and, for an accepted form, `updatedInput`
///   with either native `content` or question-UI `answers`.
///
/// # Returns
/// The native JSON-RPC reply: a selected option for a permission, or an `accept` or
/// `decline` action for a form, with validated content when accepted.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for an answer the callback does not accept,
/// including form content that violates the retained schema or repeats a unique choice.
pub fn resolve(pending: &Pending, response: &Value) -> Result<Value, AgentSessionError> {
    let Some(schema) = &pending.schema else {
        return resolve_tool(pending, response);
    };
    let action = match response["behavior"].as_str() {
        Some("allow") => "accept",
        Some("deny") => "decline",
        _ => return Err(AgentSessionError::Rejected),
    };
    let mut result = json!({"action":action});
    if action == "accept" {
        let content = form_content(schema, &response["updatedInput"])?;
        for key in &pending.unique_fields {
            if let Some(values) = content[key].as_array()
                && values
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != values.len()
            {
                return Err(AgentSessionError::Rejected);
            }
        }
        result["content"] = content;
    }
    Ok(json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":result}))
}

/// Project a supported native form; unsupported schemas reject without an implicit answer.
///
/// # Arguments
///
/// * `message` - Native `elicitation/create` callback in `form` mode.
/// * `provider` - Stable provider identity recorded on the host request.
///
/// # Returns
/// The retained callback with its normalized schema and a host question request whose
/// choices show native titles.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing native identity, and
/// `AgentSessionError::Rejected` for another mode or a schema the question UI cannot show.
pub fn form(message: &Value, provider: &str) -> Result<Pending, AgentSessionError> {
    let params = &message["params"];
    if params["mode"] != "form" {
        return Err(AgentSessionError::Rejected);
    }
    let rpc_id = message
        .get("id")
        .filter(|id| id.is_string() || id.is_number())
        .ok_or(AgentSessionError::Failed)?
        .clone();
    let schema = form_schema(&params["requestedSchema"])?;
    let unique_fields = params["requestedSchema"]["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, property)| property["uniqueItems"] == true)
        .map(|(key, _)| key.clone())
        .collect();
    let mut questions = elicitation::questions(&schema)?;
    for question in &mut questions {
        let key = question["header"]
            .as_str()
            .ok_or(AgentSessionError::Rejected)?
            .to_owned();
        let property = &schema["properties"][&key];
        question["allowEmpty"] = json!(
            !schema["required"]
                .as_array()
                .is_some_and(|keys| keys.iter().any(|value| value == &key))
        );
        let choices = if property["type"] == "array" {
            question["answerFormat"] = json!("array");
            question["answerKey"] = json!(key);
            question["allowOther"] = json!(property["items"].get("enum").is_none());
            &property["items"]
        } else {
            property
        };
        if let Some(names) = choices["enumNames"].as_array() {
            question["options"] = json!(
                names
                    .iter()
                    .map(|name| json!({"label":name}))
                    .collect::<Vec<_>>()
            );
        }
    }
    Ok(Pending {
        rpc_id,
        options: Vec::new(),
        schema: Some(schema),
        unique_fields,
        request: json!({"id":Uuid::new_v4().to_string(),"provider":provider,
            "kind":"question","name":"question","title":params["message"],
            "input":{"questions":questions},"metadata":{"toolCallId":params["toolCallId"]}}),
    })
}

fn form_schema(value: &Value) -> Result<Value, AgentSessionError> {
    let mut value = value.clone();
    let fields = value["properties"]
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?;
    for property in fields.values_mut() {
        normalize_choice(property)?;
    }
    elicitation::questions(&value)?;
    Ok(value)
}

fn normalize_choice(property: &mut Value) -> Result<(), AgentSessionError> {
    let object = property
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?;
    if let Some(choices) = object.remove("oneOf").or_else(|| object.remove("anyOf")) {
        let choices = choices
            .as_array()
            .filter(|choices| choices.len() <= 128)
            .ok_or(AgentSessionError::Rejected)?;
        let values = choices
            .iter()
            .map(|choice| {
                choice["const"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(AgentSessionError::Rejected)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let titles = choices
            .iter()
            .map(|choice| {
                choice["title"]
                    .as_str()
                    .unwrap_or(choice["const"].as_str().unwrap_or_default())
            })
            .collect::<Vec<_>>();
        object.insert("enumNames".into(), json!(titles));
        object.insert("enum".into(), json!(values));
        object.entry("type").or_insert_with(|| json!("string"));
    }
    // Retained separately for answer validation; the shared UI schema omits this keyword.
    object.remove("uniqueItems");
    if let Some(items) = object.get_mut("items") {
        normalize_choice(items)?;
    }
    Ok(())
}

fn form_content(schema: &Value, updated: &Value) -> Result<Value, AgentSessionError> {
    if updated.get("content").is_some() {
        return elicitation::content(schema, updated);
    }
    let mut updated = updated.clone();
    let answers = updated["answers"]
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?;
    for (key, value) in answers {
        let property = &schema["properties"][key];
        let choices = if property["type"] == "array" {
            &property["items"]
        } else {
            property
        };
        let convert = |value: &Value| -> Value {
            choices["enumNames"]
                .as_array()
                .and_then(|names| names.iter().position(|name| name == value))
                .and_then(|index| choices["enum"].get(index))
                .cloned()
                .unwrap_or_else(|| value.clone())
        };
        *value = if let Some(values) = value.as_array() {
            json!(values.iter().map(convert).collect::<Vec<_>>())
        } else {
            convert(value)
        };
    }
    elicitation::content(schema, &updated)
}

#[cfg(test)]
mod tests;
