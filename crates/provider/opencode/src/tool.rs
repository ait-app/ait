//! ACP tool kinds and native arguments retain the existing shell/file display cards.
use serde_json::{Value, json};

/// Project a bounded ACP tool snapshot into existing shell, file, diff or generic cards.
pub(super) fn detail(snapshot: &Value) -> Value {
    let input = &snapshot["rawInput"];
    let path = ["filePath", "file_path", "path"]
        .iter()
        .find_map(|key| input[key].as_str());
    let diff = snapshot["content"]
        .as_array()
        .and_then(|blocks| blocks.iter().find(|block| block["type"] == "diff"));
    let path = path.or_else(|| diff.and_then(|diff| diff["path"].as_str()));
    let mut detail = match snapshot["kind"].as_str() {
        Some("execute") if input["command"].is_string() => {
            json!({"type":"shell","command":input["command"]})
        }
        Some("read") if path.is_some() => json!({"type":"read","filePath":path}),
        Some("edit") if path.is_some() && input["content"].is_string() => {
            json!({"type":"write","filePath":path,"content":input["content"]})
        }
        Some("edit" | "delete") if path.is_some() => json!({"type":"edit","filePath":path}),
        Some("search") if input["query"].is_string() || input["pattern"].is_string() => {
            let query = input["query"]
                .as_str()
                .or_else(|| input["pattern"].as_str());
            json!({"type":"search","query":query,"toolName":"search"})
        }
        Some("fetch") if input["url"].is_string() => json!({"type":"fetch","url":input["url"]}),
        _ => json!({"type":"unknown","input":input,"output":null}),
    };
    for (source, target) in [
        ("cwd", "cwd"),
        ("workdir", "cwd"),
        ("offset", "offset"),
        ("limit", "limit"),
        ("old_string", "oldString"),
        ("new_string", "newString"),
        ("oldString", "oldString"),
        ("newString", "newString"),
    ] {
        if let Some(value) = input.get(source).filter(|value| match target {
            "offset" | "limit" => value.is_number(),
            _ => value.is_string(),
        }) {
            detail[target] = value.clone();
        }
    }
    if detail["type"] == "edit"
        && let Some(diff) = diff
    {
        for (source, target) in [("oldText", "oldString"), ("newText", "newString")] {
            if let Some(text) = diff[source].as_str() {
                detail[target] = json!(text);
            }
        }
    }
    let output = snapshot
        .get("rawOutput")
        .filter(|output| !output.is_null())
        .unwrap_or(&snapshot["content"]);
    match detail["type"].as_str() {
        Some("unknown") => detail["output"] = output.clone(),
        Some("shell" | "read" | "search" | "fetch") => {
            let text = content_text(&snapshot["content"])
                .or_else(|| raw_output(output).as_str().map(str::to_owned));
            if let Some(text) = text {
                let field = match detail["type"].as_str() {
                    Some("read" | "search") => "content",
                    Some("fetch") => "result",
                    _ => "output",
                };
                detail[field] = json!(text);
            }
        }
        _ => {}
    }
    detail
}

/// Borrow native `OpenCode`'s wrapped text output before bounding its display preview.
/// Structured non-text results remain intact for generic tool cards.
pub(super) fn raw_output(value: &Value) -> &Value {
    value
        .get("output")
        .filter(|output| output.is_string())
        .unwrap_or(value)
}

/// Extract ACP display text without stringifying tool metadata or non-text content.
pub(super) fn content_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    if let Some(blocks) = value.as_array() {
        let mut text = String::new();
        for part in blocks.iter().filter_map(|block| {
            if block["type"] == "content" && block["content"]["type"] == "text" {
                block["content"]["text"].as_str()
            } else {
                None
            }
        }) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part);
        }
        return (!text.is_empty()).then_some(text);
    }
    None
}

#[cfg(test)]
mod tests;
