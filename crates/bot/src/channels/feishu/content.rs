use serde_json::Value;

/// Extract only readable message content, never callback values or executable card actions.
pub(super) fn text(kind: &str, content: &Value) -> Option<String> {
    if !matches!(kind, "text" | "post" | "interactive") {
        return None;
    }
    let mut parts = Vec::new();
    collect(content, &mut parts, 0);
    let mut text = parts.join("\n");
    if text.trim().is_empty() {
        return None;
    }
    if text.len() > 15 * 1024 {
        let mut end = 15 * 1024;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[Message truncated]");
    }
    Some(text.replace('\0', ""))
}

fn collect(value: &Value, parts: &mut Vec<String>, depth: usize) {
    if depth > 32 || parts.len() >= 4096 {
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect(item, parts, depth + 1);
            }
        }
        Value::Object(object) => {
            for key in [
                "title",
                "text",
                "content",
                "content_v2",
                "elements",
                "fields",
                "zh_cn",
                "en_us",
            ] {
                if (key == "content" && object.contains_key("content_v2"))
                    || (key == "en_us" && object.contains_key("zh_cn"))
                {
                    continue;
                }
                if let Some(item) = object.get(key) {
                    if let Some(text) = item.as_str() {
                        parts.push(text.to_owned());
                    } else {
                        collect(item, parts, depth + 1);
                    }
                }
            }
            if object.get("tag").and_then(Value::as_str) == Some("at")
                && let Some(name) = object
                    .get("user_name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .or_else(|| object.get("user_id").and_then(Value::as_str))
            {
                parts.push(format!("@{name}"));
            }
            if let Some(link) = object.get("href").and_then(Value::as_str) {
                parts.push(link.to_owned());
            }
            if object.get("tag").and_then(Value::as_str) == Some("img") {
                parts.push("[Image]".into());
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

#[cfg(test)]
mod tests;
