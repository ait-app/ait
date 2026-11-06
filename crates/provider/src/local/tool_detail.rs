//! Native tool display projections. Execution and the complete raw output stay provider-owned.

use std::borrow::Cow;

use serde_json::{Value, json};

const PREVIEW_BYTES: usize = 128 * 1024;
/// Per-field budget for copied Claude text arguments such as file contents and edits.
/// Two fields and a result preview stay below the Claude stream's 192 KiB item limit.
const CLAUDE_FIELD_BYTES: usize = 32 * 1024;
/// Budget for unrecognized Claude tool arguments and for their results.
const CLAUDE_UNKNOWN_BYTES: usize = 64 * 1024;
/// Permission prompt arguments that fit this budget are shown unchanged.
const PERMISSION_INPUT_BYTES: usize = 64 * 1024;
/// Per-field budget once permission prompt arguments exceed their budget.
const PERMISSION_FIELD_BYTES: usize = 16 * 1024;
/// Notice ending a shortened display preview whose complete value stays in the native transcript.
pub(super) const MARKER: &str =
    "\n[Output truncated; full output remains in the native transcript.]";

mod command_actions;
mod mcp;
mod subagent;

/// Project a native tool into display rows, keeping command actions and file edits in source order.
///
/// `native` is a Codex thread item with a validated ID; `status` is its canonical
/// running, completed, or failed state. Returns stable call IDs paired with timeline
/// items. Compound actions share one command result identified by an `:output` suffix.
/// File edits retain the native ID for the first file and use `:file:N` for subsequent files,
/// so an initial placeholder is replaced when paths arrive. Invalid metadata falls back to one row.
pub(super) fn codex_tools(native: &Value, status: &str) -> Vec<(String, Value)> {
    let details = file_changes(native)
        .or_else(|| command_actions::details(native))
        .unwrap_or_else(|| vec![codex(native)]);
    let file_edits = native["type"] == "fileChange";
    let multiple = details.len() > 1;
    let output_index = details.len() - 1;
    let native_id = native["id"].as_str().unwrap_or_default();
    let name = codex_name(native);
    details
        .into_iter()
        .enumerate()
        .map(|(index, detail)| {
            let id = if file_edits && index > 0 {
                format!("{native_id}:file:{index}")
            } else if file_edits {
                native_id.to_owned()
            } else if multiple && index == output_index {
                format!("{native_id}:output")
            } else if multiple {
                format!("{native_id}:{index}")
            } else {
                native_id.to_owned()
            };
            let error = codex_error(native, status == "failed");
            let item = json!({"type":"tool_call","callId":id,"name":name,
                "status":status,"error":error,"detail":detail});
            (id, item)
        })
        .collect()
}

/// Return the display name for `native`, including its MCP server when present.
///
/// Borrows the native type or tool name where possible; allocates only for `server.tool`.
pub(super) fn codex_name(native: &Value) -> Cow<'_, str> {
    if native["type"] == "subAgentActivity" {
        return Cow::Borrowed("Sub-agent");
    }
    if native["type"] == "mcpToolCall"
        && let Some(tool) = native["tool"]
            .as_str()
            .map(str::trim)
            .filter(|tool| !tool.is_empty())
    {
        let server = native["server"].as_str().map(str::trim).unwrap_or_default();
        return if server.is_empty() {
            Cow::Borrowed(tool)
        } else {
            Cow::Owned(format!("{server}.{tool}"))
        };
    }
    Cow::Borrowed(native["type"].as_str().unwrap_or_default())
}

/// Return the error for `native` when `failed`, or null for a successful tool.
///
/// MCP failures retain their native error; other failures use a generic message.
pub(super) fn codex_error(native: &Value, failed: bool) -> Value {
    if !failed {
        return Value::Null;
    }
    if native["type"] == "mcpToolCall" && !native["error"].is_null() {
        return mcp::error(&native["error"]);
    }
    json!("Native tool failed")
}

/// Project recognized native arguments into typed display fields.
/// Process resources retain their native input instead of appearing as filesystem operations.
///
/// Copied text arguments such as commands, file contents, edits and plans are bounded display
/// previews; complete arguments remain in the native transcript.
pub(super) fn claude(name: &str, input: &Value) -> Value {
    if matches!(name, "Read" | "Edit" | "Write")
        && input["file_path"]
            .as_str()
            .is_some_and(|path| path.starts_with("proc://"))
    {
        return claude_unknown(input);
    }
    let mut detail = match name {
        "Bash" if input["command"].is_string() => json!({"type":"shell"}),
        "Read" if input["file_path"].is_string() => {
            json!({"type":"read","filePath":input["file_path"]})
        }
        "Edit" if input["file_path"].is_string() => {
            json!({"type":"edit","filePath":input["file_path"]})
        }
        "Write" if input["file_path"].is_string() => {
            json!({"type":"write","filePath":input["file_path"]})
        }
        "Glob" | "Grep" => {
            json!({"type":"search","toolName":if name=="Glob" {"glob"} else {"grep"},"query":input["pattern"].as_str().unwrap_or("")})
        }
        "WebSearch" => {
            json!({"type":"search","toolName":"web_search","query":input["query"].as_str().unwrap_or("")})
        }
        "WebFetch" if input["url"].is_string() => json!({"type":"fetch","url":input["url"]}),
        "Agent" | "Task" | "Workflow" => json!({"type":"sub_agent","log":"","actions":[]}),
        "ExitPlanMode" if input["plan"].is_string() => json!({"type":"plan"}),
        _ => return claude_unknown(input),
    };
    for (source, target) in [
        ("command", "command"),
        ("plan", "text"),
        ("cwd", "cwd"),
        ("old_string", "oldString"),
        ("new_string", "newString"),
        ("content", "content"),
        ("prompt", "prompt"),
        ("description", "description"),
        ("subagent_type", "subAgentType"),
    ] {
        if let Some(value) = input.get(source).and_then(Value::as_str) {
            detail[target] = json!(truncate(value, CLAUDE_FIELD_BYTES, MARKER));
        }
    }
    for field in ["offset", "limit"] {
        if let Some(value) = input.get(field).filter(|value| value.is_number()) {
            detail[field] = value.clone();
        }
    }
    detail
}

/// Add a bounded display preview of a Claude tool `result` to its typed `detail`.
pub(super) fn claude_result(detail: &mut Value, result: &Value) {
    let field = match detail["type"].as_str() {
        Some("shell") => "output",
        Some("read" | "search") => "content",
        Some("fetch") => "result",
        Some("sub_agent") => "log",
        Some("unknown") => {
            detail["output"] = match result.as_str() {
                Some(text) => json!(truncate(text, CLAUDE_UNKNOWN_BYTES, MARKER)),
                None => mcp::bounded(result, CLAUDE_UNKNOWN_BYTES),
            };
            return;
        }
        _ => return,
    };
    detail[field] = json!(output_text(result));
}

/// Project native tool arguments for a host permission prompt within a display budget.
///
/// # Arguments
///
/// * `input` - Complete native arguments; callers retain them to answer the native request.
///
/// # Returns
///
/// `input` unchanged when it fits. Larger objects keep every top-level field, with marked text
/// prefixes and marked JSON previews, so the prompt still names the path or command it approves.
/// Any other oversized shape becomes one marked JSON preview.
pub(super) fn input_preview(input: &Value) -> Value {
    let bounded = mcp::bounded(input, PERMISSION_INPUT_BYTES);
    let (true, Some(fields)) = (bounded.is_string(), input.as_object()) else {
        return bounded;
    };
    let projected = fields
        .iter()
        .map(|(key, value)| {
            let preview = match value.as_str() {
                Some(text) => json!(truncate(text, PERMISSION_FIELD_BYTES, MARKER)),
                None => mcp::bounded(value, PERMISSION_FIELD_BYTES),
            };
            (key.clone(), preview)
        })
        .collect();
    // Many fields can still exceed the prompt budget; bound the whole projection as a last resort.
    mcp::bounded(&Value::Object(projected), 2 * PERMISSION_INPUT_BYTES)
}

fn claude_unknown(input: &Value) -> Value {
    json!({"type":"unknown","input":mcp::bounded(input, CLAUDE_UNKNOWN_BYTES),"output":null})
}

pub(super) fn codex(native: &Value) -> Value {
    match native["type"].as_str() {
        Some("commandExecution") => {
            command_actions::shell(native, native["command"].as_str().unwrap_or(""), true)
        }
        Some("fileChange") => {
            let detail = native["changes"].as_array().and_then(|changes| {
                let [change] = changes.as_slice() else {
                    return None;
                };
                file_change(change)
            });
            detail.unwrap_or_else(|| unknown(native))
        }
        Some("webSearch") => {
            if let Some(url) = native["action"]["url"].as_str() {
                json!({"type":"fetch","url":url})
            } else {
                json!({"type":"search","toolName":"web_search","query":native["query"].as_str().or_else(||native["action"]["query"].as_str()).unwrap_or("")})
            }
        }
        Some("collabAgentToolCall") => {
            let mut detail = json!({"type":"sub_agent","description":native["prompt"].as_str().unwrap_or(""),
                "log":native["tool"].as_str().unwrap_or(""),"actions":[]});
            if let Some(id) = native["receiverThreadIds"]
                .as_array()
                .and_then(|ids| ids.first())
                .and_then(Value::as_str)
            {
                detail["childSessionId"] = json!(id);
            }
            detail
        }
        Some("subAgentActivity") => subagent::activity(native).unwrap_or_else(|| unknown(native)),
        Some("mcpToolCall") => mcp::detail(native),
        Some("imageView" | "imageGeneration") => {
            json!({"type":"plain_text","label":"Image","text":native["path"].as_str().unwrap_or("")})
        }
        _ => unknown(native),
    }
}

fn file_changes(native: &Value) -> Option<Vec<Value>> {
    if native["type"] != "fileChange" {
        return None;
    }
    let changes = native["changes"].as_array()?;
    changes.first()?;
    changes.iter().map(file_change).collect()
}

fn file_change(change: &Value) -> Option<Value> {
    let path = change["path"].as_str().filter(|path| !path.is_empty())?;
    let diff = change["diff"].as_str().unwrap_or_default();
    Some(json!({"type":"edit","filePath":path,"unifiedDiff":preview(diff)}))
}

fn unknown(native: &Value) -> Value {
    json!({"type":"unknown","input":native,"output":null})
}

fn output_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return preview(text).into_owned();
    }
    if let Some(blocks) = value.as_array() {
        let mut output = String::new();
        for block in blocks {
            if output.len() >= PREVIEW_BYTES {
                break;
            }
            if let Some(text) = block["text"].as_str() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&preview(text));
            }
        }
        return preview(&output).into_owned();
    }
    preview(&value.to_string()).into_owned()
}

fn preview(text: &str) -> Cow<'_, str> {
    truncate(text, PREVIEW_BYTES, MARKER)
}

/// Bound `text` to `limit` bytes, ending a shortened prefix with `marker`.
///
/// # Arguments
///
/// * `text` - Complete display text.
/// * `limit` - Maximum UTF-8 byte length of the result, including `marker`.
/// * `marker` - Notice appended when `text` is shortened; it should fit within `limit`.
///
/// # Returns
///
/// `text` unchanged when it fits, otherwise its longest char-aligned prefix plus `marker`.
pub(super) fn truncate<'a>(text: &'a str, limit: usize, marker: &str) -> Cow<'a, str> {
    if text.len() <= limit {
        return Cow::Borrowed(text);
    }
    let end = text.floor_char_boundary(limit.saturating_sub(marker.len()));
    Cow::Owned(format!("{}{marker}", &text[..end]))
}

#[cfg(test)]
mod tests;
