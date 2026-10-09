use serde_json::{Value, json};
use uuid::Uuid;

use super::{PROVIDER, config::text};
use crate::ports::agent_session::AgentSessionError;

/// Correlated native callback retained until exactly one validated response is flushed.
#[derive(Debug)]
pub(super) struct Pending {
    pub(super) rpc_id: Value,
    pub(super) request: Value,
    options: Vec<Value>,
    schema: Option<Value>,
    unique_fields: Vec<String>,
}

/// Project native permission choices; malformed identities and option lists fail closed.
pub(super) fn capture(message: &Value) -> Result<Pending, AgentSessionError> {
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
        schema: None,
        unique_fields: Vec::new(),
        options: options.clone(),
        request: json!({"id":Uuid::new_v4().to_string(),"provider":PROVIDER,"name":title,
            "kind":"tool","title":title,"input":params["toolCall"]["rawInput"],"actions":actions,
            "detail":{"type":"unknown","input":params["toolCall"]["rawInput"],"output":null},
            "metadata":{"toolCallId":params["toolCall"]["toolCallId"]}}),
    })
}

/// Validate a host answer and build its exact native callback reply; invalid answers reject.
pub(super) fn resolve(pending: &Pending, response: &Value) -> Result<Value, AgentSessionError> {
    if let Some(schema) = &pending.schema {
        let action = match response["behavior"].as_str() {
            Some("allow") => "accept",
            Some("deny") => "decline",
            _ => return Err(AgentSessionError::Rejected),
        };
        let mut result = json!({"action":action});
        if action == "accept" {
            let content = answer(schema, &response["updatedInput"])?;
            for key in &pending.unique_fields {
                if let Some(values) = content[key].as_array()
                    && values
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != values.len()
                {
                    return Err(AgentSessionError::Rejected);
                }
            }
            result["content"] = content;
        }
        return Ok(json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":result}));
    }
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

/// Project a supported native form; unsupported schemas reject without an implicit answer.
pub(super) fn form(message: &Value) -> Result<Pending, AgentSessionError> {
    let params = &message["params"];
    if params["mode"] != "form" {
        return Err(AgentSessionError::Rejected);
    }
    let rpc_id = message
        .get("id")
        .filter(|id| id.is_string() || id.is_number())
        .ok_or(AgentSessionError::Failed)?
        .clone();
    let schema = schema(&params["requestedSchema"])?;
    let unique_fields = params["requestedSchema"]["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, property)| property["uniqueItems"] == true)
        .map(|(key, _)| key.clone())
        .collect();
    let mut questions = crate::local::elicitation::questions(&schema)?;
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
        request: json!({"id":Uuid::new_v4().to_string(),"provider":PROVIDER,
            "kind":"question","name":"question","title":params["message"],
            "input":{"questions":questions},"metadata":{"toolCallId":params["toolCallId"]}}),
    })
}

fn schema(value: &Value) -> Result<Value, AgentSessionError> {
    let mut value = value.clone();
    let fields = value["properties"]
        .as_object_mut()
        .ok_or(AgentSessionError::Rejected)?;
    for property in fields.values_mut() {
        normalize_choice(property)?;
    }
    crate::local::elicitation::questions(&value)?;
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

fn answer(schema: &Value, updated: &Value) -> Result<Value, AgentSessionError> {
    if updated.get("content").is_some() {
        return crate::local::elicitation::content(schema, updated);
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
    crate::local::elicitation::content(schema, &updated)
}
