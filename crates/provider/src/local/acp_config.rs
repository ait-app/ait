//! Shared ACP configuration helpers for stdio ACP adapters.

use serde_json::Value;

use crate::ports::agent_session::AgentSessionError;

/// Borrow the native select option for a protocol category, when advertised.
///
/// # Arguments
///
/// * `options` - Native `configOptions` snapshot.
/// * `category` - Protocol category such as `model` or `thought_level`.
///
/// # Returns
/// The select option whose category matches, or none when the adapter omits it.
pub(crate) fn option<'a>(options: &'a Value, category: &str) -> Option<&'a Value> {
    options
        .as_array()?
        .iter()
        .find(|option| option["category"] == category && option["type"] == "select")
}

/// Flatten native select groups within catalog bounds; malformed choices fail closed.
///
/// # Arguments
///
/// * `option` - One native select option.
///
/// # Returns
/// Every validated choice, with grouped choices flattened in declaration order.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing, oversized or malformed choice list.
pub(crate) fn choices(option: &Value) -> Result<Vec<&Value>, AgentSessionError> {
    let options = option["options"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut choices = Vec::new();
    for entry in options {
        if let Some(group) = entry["options"].as_array() {
            choices.extend(group);
        } else {
            choices.push(entry);
        }
        if choices.len() > 4096 {
            return Err(AgentSessionError::Failed);
        }
    }
    for choice in &choices {
        choice["value"]
            .as_str()
            .filter(|value| value.len() <= 1024)
            .ok_or(AgentSessionError::Failed)?;
        text(choice, "name")?;
    }
    Ok(choices)
}

/// Validate and copy a bounded native configuration response, rejecting malformed options.
///
/// # Arguments
///
/// * `response` - Native response carrying a `configOptions` array.
///
/// # Returns
/// The validated options as a JSON array safe to retain as session state.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing, oversized or malformed option list.
pub(crate) fn state(response: &Value) -> Result<Value, AgentSessionError> {
    let options = response["configOptions"]
        .as_array()
        .filter(|options| options.len() <= 128)
        .ok_or(AgentSessionError::Failed)?;
    for option in options {
        text(option, "id")?;
        if option["type"] == "select" {
            choices(option)?;
            option["currentValue"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?;
        }
    }
    Ok(Value::Array(options.clone()))
}

/// Borrow a nonempty native string field; missing, oversized or control text is a failure.
///
/// # Arguments
///
/// * `value` - Native value owning the field.
/// * `key` - Field name to read.
///
/// # Returns
/// The borrowed text when it is nonempty, at most 1024 bytes and free of control characters.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` when the field is absent or out of bounds.
pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 1024 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}
