//! Validate the small, closed JSON Schema vocabulary used by the embedded catalog.

use serde_json::Value;

/// Return whether `value` satisfies the static catalog's supported `schema` constraints.
pub(super) fn matches(schema: &Value, value: &Value) -> bool {
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.contains(value))
    {
        return false;
    }
    if let Some(types) = schema["type"].as_array() {
        return types.iter().any(|kind| matches_type(kind.as_str(), value))
            && constraints(schema, value);
    }
    matches_type(schema["type"].as_str(), value) && constraints(schema, value)
}

fn matches_type(kind: Option<&str>, value: &Value) -> bool {
    match kind {
        Some("object") => value.is_object(),
        Some("array") => value.is_array(),
        Some("string") => value.is_string(),
        Some("integer") => value.as_i64().is_some() || value.as_u64().is_some(),
        Some("boolean") => value.is_boolean(),
        Some("null") => value.is_null(),
        None => true,
        Some(_) => false,
    }
}

fn constraints(schema: &Value, value: &Value) -> bool {
    if let Some(object) = value.as_object() {
        if schema["required"].as_array().is_some_and(|required| {
            required
                .iter()
                .any(|key| key.as_str().is_none_or(|key| !object.contains_key(key)))
        }) {
            return false;
        }
        for (key, value) in object {
            if let Some(property) = schema["properties"].get(key) {
                if !matches(property, value) {
                    return false;
                }
            } else if schema["additionalProperties"] == false
                || (schema["additionalProperties"].is_object()
                    && !matches(&schema["additionalProperties"], value))
            {
                return false;
            }
        }
    }
    if let Some(array) = value.as_array()
        && schema
            .get("items")
            .is_some_and(|item| array.iter().any(|value| !matches(item, value)))
    {
        return false;
    }
    if let Some(text) = value.as_str() {
        let length = text.chars().count();
        if schema["minLength"]
            .as_u64()
            .is_some_and(|min| length < usize::try_from(min).unwrap_or(usize::MAX))
            || schema["maxLength"]
                .as_u64()
                .is_some_and(|max| length > usize::try_from(max).unwrap_or(usize::MAX))
        {
            return false;
        }
    }
    if let Some(number) = value.as_f64()
        && (schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max))
    {
        return false;
    }
    true
}
