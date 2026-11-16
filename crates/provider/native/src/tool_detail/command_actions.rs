use serde_json::{Value, json};

use super::preview;

// Bound action metadata without dropping the command: excessive or malformed lists
// use the raw shell row. Compound stdout belongs only to the original command.
const MAX_ACTIONS: usize = 256;

/// Return action details and one shared command result, or `None` to retain the raw command.
///
/// `native` is a Codex thread item. Non-command items, declined commands, and absent,
/// malformed, or excessive action lists return `None` without partially projecting.
/// Single actions own the native output; multiple actions retain metadata only and
/// append the original command as the sole owner of stdout and exit status.
pub(super) fn details(native: &Value) -> Option<Vec<Value>> {
    if native["type"] != "commandExecution" || native["status"] == "declined" {
        return None;
    }
    let actions = native["commandActions"].as_array()?;
    if actions.is_empty() || actions.len() > MAX_ACTIONS {
        return None;
    }
    let single = actions.len() == 1;
    let mut details = actions
        .iter()
        .map(|action| detail(native, action, single))
        .collect::<Option<Vec<_>>>()?;
    if !single {
        details.push(shell(
            native,
            native["command"].as_str().unwrap_or(""),
            true,
        ));
    }
    Some(details)
}

fn detail(native: &Value, action: &Value, owns_output: bool) -> Option<Value> {
    let command = action["command"].as_str()?.trim();
    let mut detail = match action["type"].as_str()? {
        "read" => json!({"type":"read","filePath":action["path"].as_str()?}),
        "listFiles" => json!({"type":"search","toolName":"glob",
            "query":action["path"].as_str().unwrap_or(command)}),
        "search" => {
            let mut detail = json!({"type":"search","toolName":"grep",
                "query":action["query"].as_str().unwrap_or(command)});
            if let Some(path) = action["path"].as_str() {
                detail["filePaths"] = json!([path]);
            }
            detail
        }
        _ => return Some(shell(native, command, owns_output)),
    };
    detail["content"] = if owns_output {
        json!(preview(native["aggregatedOutput"].as_str().unwrap_or("")))
    } else {
        json!("")
    };
    Some(detail)
}

/// Keep the selected command together with its native working directory and output.
///
/// `native` supplies optional cwd, output, and exit status; `command` is either the
/// authoritative parsed action or the raw fallback. `owns_output` distinguishes a
/// command result from a compound action summary. Returns a shell detail object.
pub(super) fn shell(native: &Value, command: &str, owns_output: bool) -> Value {
    let mut detail = json!({"type":"shell","command":command,"output":""});
    if let Some(cwd) = native["cwd"].as_str() {
        detail["cwd"] = json!(cwd);
    }
    if owns_output {
        detail["output"] = json!(preview(native["aggregatedOutput"].as_str().unwrap_or("")));
        if let Some(exit) = native["exitCode"].as_i64() {
            detail["exitCode"] = json!(exit);
        }
    }
    detail
}
