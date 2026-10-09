//! ACP tool kinds and native arguments retain the existing shell/file display cards.
use serde_json::{Value, json};

/// Project a bounded ACP tool snapshot into existing shell, file, diff or generic cards.
pub(super) fn detail(snapshot: &Value) -> Value {
    let input = &snapshot["rawInput"];
    let path = ["filePath", "file_path", "path"]
        .iter()
        .find_map(|key| input[key].as_str());
    let mut detail = match snapshot["kind"].as_str() {
        Some("execute") if input["command"].is_string() => {
            json!({"type":"shell","command":input["command"]})
        }
        Some("read") if path.is_some() => json!({"type":"read","filePath":path}),
        Some("edit") if path.is_some() => json!({"type":"edit","filePath":path}),
        Some("search") => {
            json!({"type":"search","query":input.get("query").unwrap_or(&input["pattern"]),"toolName":snapshot["title"]})
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
        if let Some(value) = input.get(source) {
            detail[target] = value.clone();
        }
    }
    let output = snapshot.get("rawOutput").unwrap_or(&snapshot["content"]);
    let field = if detail["type"] == "read" {
        "content"
    } else if detail["type"] == "fetch" {
        "result"
    } else {
        "output"
    };
    detail[field] = output.clone();
    detail
}

#[cfg(test)]
mod tests;
