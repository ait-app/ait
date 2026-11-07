//! Keep host-owned controls and unavailable TUI commands out of native prompt dispatch.

use serde_json::{Value, json};

pub(super) mod pending;

/// Builtins whose text handlers preserve Ait's model, permission and conversation ownership.
const EXPORTED: &[&str] = &[
    "advisor",
    "export",
    "dump",
    "todo",
    "session",
    "jobs",
    "usage",
    "changelog",
    "tools",
    "context",
    "mcp",
    "compact",
    "retry",
    "memory",
    "pin",
    "marketplace",
    "plugins",
    "reload-plugins",
    "add-dir",
    "remove-dir",
    "dirs",
    "forget-approvals",
    "skillful",
    "extended-context",
    "computer",
    "ratchet",
];

/// TUI-only names and aliases from official OMP 18.14.0, never ordinary model prompts.
const TUI_ONLY: &[&str] = &[
    "collab",
    "join",
    "leave",
    "copy",
    "open",
    "live",
    "record",
    "pause",
    "quit",
    "q",
    "new",
    "clear",
    "delete",
    "resume",
    "tan",
    "seance",
    "omfg",
    "cleanse",
    "debug",
    "exit",
    "restart",
    "settings",
    "setup",
    "providers",
    "plan",
    "plan-review",
    "goal",
    "guided-goal",
    "loop",
    "queue",
    "hotkeys",
    "extensions",
    "status",
    "agents",
    "git",
    "local",
    "hub",
    "subagents",
    "branch",
    "rewind",
    "fork",
    "tree",
    "login",
    "logout",
    "skills",
];

const MANAGED: &[&str] = &[
    "security",
    "model",
    "models",
    "switch",
    "fast",
    "slow",
    "modelpreset",
    "effort",
    "share",
    "browser",
    "stats",
    "trace",
    "ssh",
    "fresh",
    "shake",
    "handoff",
    "rename",
    "move",
    "wt",
    "worktree",
    "force",
    "prewalk",
];

/// Split a native slash name without treating absolute paths as commands.
pub(super) fn split(text: &str) -> Option<(&str, &str)> {
    let input = text.trim().strip_prefix('/')?;
    let end = input
        .find(|c: char| c.is_whitespace() || c == ':')
        .unwrap_or(input.len());
    let name = &input[..end];
    (!name.is_empty() && !name.contains('/'))
        .then_some((name, input[end..].trim_start_matches(':').trim()))
}

/// Recognize adapter controls before foreground admission, including manually typed blocked names.
pub(super) fn is_btw(text: &str) -> bool {
    split(text).is_some_and(|(name, _)| name == "btw")
}

/// Recognize the native Vibe control, which the host owns once the adapter announces it.
pub(super) fn is_vibe(text: &str) -> bool {
    split(text).is_some_and(|(name, _)| name == "vibe")
}

/// Only query forms of known builtin handlers may execute beside a foreground model turn.
pub(super) fn read_only(input: &str, native: &[Value]) -> bool {
    let Some((name, args)) = split(input) else {
        return false;
    };
    let Some(entry) = native.iter().find(|entry| matches_name(entry, name)) else {
        return false;
    };
    if entry["source"] != "builtin" {
        return false;
    }
    query_form(entry["name"].as_str().unwrap_or_default(), args)
}

/// Known query forms can be admitted before an inactive owner is resumed.
pub(super) fn is_query(input: &str) -> bool {
    split(input).is_some_and(|(name, args)| query_form(name, args))
}

fn query_form(name: &str, args: &str) -> bool {
    match name {
        "usage" => matches!(args, "" | "show"),
        "advisor" => matches!(args, "status" | "dump"),
        "session" => matches!(args, "" | "info"),
        "context" | "tools" | "dirs" | "todo" => args.is_empty(),
        "jobs" => matches!(args, "" | "full"),
        "mcp" => matches!(args, "" | "list" | "status"),
        "plugins" | "marketplace" => matches!(args, "" | "list"),
        "memory" => matches!(args, "" | "view" | "stats" | "diagnose" | "queue"),
        "changelog" => {
            args.is_empty()
                || args == "full"
                || args == "last"
                || args
                    .strip_prefix("last ")
                    .is_some_and(|n| n.parse::<u16>().is_ok())
        }
        _ => false,
    }
}

/// Identify a native builtin whose uncorrelated output must finish before another text query.
pub(super) fn builtin(input: &str, native: &[Value]) -> bool {
    split(input).is_some_and(|(name, _)| {
        native
            .iter()
            .any(|entry| entry["source"] == "builtin" && matches_name(entry, name))
    })
}

/// A control result remains separate from the foreground assistant's final answer.
pub(super) fn note(
    id: &str,
    command: &str,
    failed: bool,
    text: &str,
) -> crate::protocol::timeline::NativeItem {
    crate::protocol::timeline::NativeItem {
        key: format!("host:provider-command:omp:{id}"),
        turn_id: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        item: json!({"type":"provider_command", "id":id,
            "command":command,"status":if failed {"error"} else {"success"},"text":text}),
    }
}

/// Check the current native catalog first so a real extension may own a TUI-only name.
///
/// `vibe` is executable only when the native host announced the bridge *and* its catalog
/// advertises a builtin `vibe`: the greeting alone says the runtime is patched, the catalog
/// entry is the proof that the adapter actually wired the command table (a plain `--mode rpc`
/// launch advertises no `vibe` entry at all). Until both hold the name stays a host refusal,
/// so it can never silently become a model prompt.
pub(super) fn blocked<'a>(text: &'a str, native: &[Value], bridge: bool) -> Option<&'a str> {
    let (name, args) = split(text)?;
    if name == "btw" {
        return None;
    }
    if let Some(entry) = native.iter().find(|entry| matches_name(entry, name)) {
        // A real extension, skill or file command owns its name, bridge or not.
        if entry["source"] != "builtin" {
            return None;
        }
        if name == "vibe" {
            return (!bridge || !bridged(native)).then_some(name);
        }
        return (!exported(entry)
            || !subcommand_exported(entry["name"].as_str().unwrap_or_default(), args))
        .then_some(name);
    }
    if name == "vibe" {
        return (!bridge || !bridged(native)).then_some(name);
    }
    (TUI_ONLY.contains(&name) || MANAGED.contains(&name)).then_some(name)
}

/// The adapter advertises a builtin `/vibe` only when its command table is wired.
fn bridged(native: &[Value]) -> bool {
    native
        .iter()
        .any(|entry| matches_name(entry, "vibe") && entry["source"] == "builtin")
}

fn matches_name(entry: &Value, name: &str) -> bool {
    entry["name"] == name
        || entry["aliases"].as_array().is_some_and(|aliases| {
            aliases.iter().any(|alias| {
                alias
                    .as_str()
                    .is_some_and(|alias| alias.trim_end_matches(':') == name)
            })
        })
}

fn exported(entry: &Value) -> bool {
    entry["name"]
        .as_str()
        .is_some_and(|name| EXPORTED.contains(&name))
}

fn subcommand_exported(name: &str, args: &str) -> bool {
    let verb = args.split_whitespace().next().unwrap_or_default();
    match name {
        "advisor" => verb != "configure",
        "session" => !matches!(verb, "delete" | "pin"),
        "todo" => !matches!(verb, "edit" | "expand" | "collapse"),
        "mcp" => !matches!(
            verb,
            "unauth" | "smithery-login" | "smithery-logout" | "reconnect" | "notifications"
        ),
        _ => true,
    }
}

/// Project only supported builtin handlers, retaining extension/skill/file commands verbatim.
pub(super) fn visible(native: &[Value]) -> Vec<Value> {
    let mut visible: Vec<_> = native
        .iter()
        .filter(|entry| entry["source"] != "builtin" || exported(entry))
        .map(|entry| {
            let name = entry["name"].as_str().unwrap_or_default();
            let mut entry = entry.clone();
            if entry["source"] == "builtin"
                && let Some(subcommands) =
                    entry.get_mut("subcommands").and_then(Value::as_array_mut)
            {
                subcommands.retain(|command| {
                    subcommand_exported(name, command["name"].as_str().unwrap_or_default())
                });
            }
            entry
        })
        .collect();
    if !visible.iter().any(|entry| entry["name"] == "btw") {
        visible.push(json!({"name":"btw","description":"Ask a native side question without interrupting the main turn; omit the question to view history",
            "argumentHint":"[question]","kind":"command","source":"builtin"}));
    }
    visible
}

/// Explain unavailable native behavior without replacing Ait's own controls or invoking a model.
pub(super) fn rejection(name: &str) -> String {
    if name == "vibe" {
        return "OMP /vibe requires the Ait native host bridge. This runtime did not announce \
                bridgeVersion 1 with the vibe capability, so the native Vibe mode is unavailable. \
                Install the current OMP native host runtime, or use Ait's subagent controls."
            .to_owned();
    }
    if TUI_ONLY.contains(&name) {
        format!(
            "OMP /{name} requires its terminal UI and is not exposed through RPC. Use the corresponding Ait control or native operation."
        )
    } else {
        format!(
            "OMP /{name} changes a control owned by Ait or opens an external interface. Use Ait's model, permission, workspace or native-operation controls instead."
        )
    }
}

#[cfg(test)]
mod tests;
