//! Keep host-owned controls and unavailable TUI commands out of native prompt dispatch.

use serde_json::{Value, json};

pub(super) mod pending;

/// Builtins whose text handlers preserve Ait's model, permission and conversation ownership.
const EXPORTED: &[&str] = &[
    "usage",
    "changelog",
    "tools",
    "context",
    "jobs",
    "dirs",
    "session",
    "memory",
    "security",
    "compact",
    "handoff",
    "force",
    "ratchet",
];

/// Terminal-only builtins have no corresponding Ait action.
const TUI_ONLY: &[&str] = &["record", "hotkeys", "quit", "q", "exit", "debug"];

/// GUI-owned builtin names and aliases. Native interfaces remain callable by their owning GUI.
const MANAGED: &[&str] = &[
    "model",
    "models",
    "switch",
    "modelpreset",
    "effort",
    "agents",
    "prewalk",
    "advisor",
    "fast",
    "slow",
    "skillful",
    "extended-context",
    "computer",
    "browser",
    "settings",
    "setup",
    "providers",
    "login",
    "logout",
    "todo",
    "hub",
    "subagents",
    "plan-review",
    "new",
    "clear",
    "delete",
    "fresh",
    "resume",
    "pin",
    "unpin",
    "rename",
    "move",
    "wt",
    "worktree",
    "branch",
    "rewind",
    "fork",
    "tree",
    "retry",
    "restart",
    "shake",
    "mcp",
    "skills",
    "marketplace",
    "plugins",
    "plugin",
    "reload-plugins",
    "ssh",
    "forget-approvals",
    "add-dir",
    "remove-dir",
    "export",
    "dump",
    "copy",
    "open",
    "local",
    "git",
    "share",
    "collab",
    "join",
    "leave",
    "pause",
    "live",
    "annotate",
    // These task flows must be dispatched by a host adapter, never ordinary inference.
    "plan",
    "goal",
    "guided-goal",
    "loop",
    "queue",
    "tan",
    "omfg",
    "cleanse",
    "extensions",
    "status",
    "stats",
    "trace",
    "seance",
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
        "memory" => {
            matches!(args, "" | "view" | "stats" | "diagnose" | "queue")
                || args.strip_prefix("mm ").is_some_and(|args| {
                    matches!(
                        args.split_whitespace().next(),
                        Some("list" | "show" | "history")
                    )
                })
        }
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
/// Legacy native Vibe requires both bridge negotiation and its builtin catalog entry.
/// The external-OMP coordinator handles its announced task controls before this fallback.
/// A real project/extension owner always retains native dispatch priority.
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
    match name {
        "session" => matches!(args.trim(), "" | "info"),
        "memory" => {
            let mut words = args.split_whitespace();
            match words.next().unwrap_or_default() {
                "" | "view" | "stats" | "diagnose" | "queue" => true,
                "mm" => matches!(words.next(), Some("list" | "show" | "history")),
                _ => false,
            }
        }
        "usage" => matches!(args.trim(), "" | "show"),
        _ => true,
    }
}

/// Only the effective native owner may trigger a host-special command, including aliases.
/// Missing builtins retain host fallback; project/extension commands keep their native semantics.
pub(super) fn owns_control(text: &str, native: &[Value], control: &str) -> bool {
    let Some((name, _)) = split(text) else {
        return false;
    };
    if name != control {
        return false;
    }
    native
        .iter()
        .find(|entry| matches_name(entry, name))
        .is_none_or(|entry| entry["source"] == "builtin")
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
        return "OMP /vibe requires a compatible external native host bridge. The installed OMP \
                did not announce bridgeVersion 1 with the vibe capability and a builtin vibe \
                command, so native Vibe mode is unavailable. Ait does not install or modify OMP. \
                Ait's subagent controls remain available."
            .to_owned();
    }
    if TUI_ONLY.contains(&name) {
        return format!("/{name} is available only in the OMP terminal.");
    }
    let location = match name {
        "model" | "models" | "switch" => "Ait's model selector",
        "effort" => "Ait's thinking-level control",
        "modelpreset" => "Model Presets in the model selector's provider settings",
        "agents" | "advisor" | "prewalk" => "provider role settings",
        "login" | "logout" | "session" => "the corresponding session or provider account controls",
        "fast" | "slow" => "Ait's service-tier control",
        "mcp" | "skills" | "marketplace" | "plugins" | "plugin" | "reload-plugins" => {
            "provider integration settings"
        }
        _ => "the corresponding Ait session, workspace or provider control",
    };
    format!("Use {location}. The builtin /{name} command is disabled in Ait.")
}

#[cfg(test)]
mod tests;
