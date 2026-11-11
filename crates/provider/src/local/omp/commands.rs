//! Provenance-aware command policy: builtin filtering never intercepts a real plugin,
//! skill, template or project owner, and blocked builtins never reach navigation or a model.

use serde_json::{Value, json};

/// Terminal-only builtins have no corresponding Ait action (frozen command dispositions:
/// /hotkeys, /quit, /q, /exit, /record, /debug are excluded terminal UI/exit commands).
const TUI_ONLY: &[&str] = &["record", "hotkeys", "quit", "q", "exit", "debug"];

/// GUI-owned builtin names and aliases: an existing Ait interface owns these facts, so the
/// native slash management/navigation entry is refused without navigation or model forwarding.
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
    "fresh",
    "pin",
    "unpin",
    "rename",
    "move",
    "wt",
    "worktree",
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
];

/// Incomplete native workflows and host-owned task flows whose Ait delivery belongs to a later
/// split task. They are explicitly unavailable, never forwarded to ordinary inference, and no
/// replacement GUI is implied. Working native handlers outside these lists (for example
/// /compact, /usage, /session, /memory) keep their existing supported execution.
/// `handoff` is included because a builtin handoff can change native session identity or
/// history without task25's atomic reconciliation; a real plugin owner still keeps dispatch.
const UNDELIVERED: &[&str] = &[
    "btw",
    "vibe",
    "goal",
    "guided-goal",
    "plan",
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
    "handoff",
    // History-mutating builtins stay unavailable until task25 owns atomic identity
    // reconciliation; official OMP 18.17.0 does not ship them, but other runtimes might.
    "new",
    "clear",
    "delete",
    "resume",
    "branch",
    "rewind",
    "fork",
    "tree",
];

fn blocked_name(name: &str) -> bool {
    TUI_ONLY.contains(&name) || MANAGED.contains(&name) || UNDELIVERED.contains(&name)
}

/// Split a native slash name without treating absolute paths as commands.
pub(super) fn split(text: &str) -> Option<(&str, &str)> {
    let input = text.trim().strip_prefix('/')?;
    let end = input
        .find(|c: char| c.is_whitespace() || c == ':')
        .unwrap_or(input.len());
    let name = &input[..end];
    (!name.is_empty() && !name.contains('/'))
        .then(|| (name, input[end..].trim_start_matches(':').trim_start()))
}

/// Check the current native catalog first so a real extension may own a blocked name.
/// Returns the blocked builtin name, or `None` when the text may keep native dispatch.
/// Filtering requires an explicit builtin source: an entry with another or missing source
/// keeps native ownership, and a name absent from the catalog is never assumed builtin.
pub(super) fn blocked<'a>(text: &'a str, native: &[Value]) -> Option<&'a str> {
    let (name, _) = split(text)?;
    blockable(name, native).then_some(name)
}

/// Whether the bare command `name` resolves to a refused explicit builtin in this catalog.
fn blockable(name: &str, native: &[Value]) -> bool {
    blocked_name(name) && builtin_owned(native, name)
}

/// Whether every catalog claimant of `name` is an explicit builtin owner.
/// A real extension, skill, template or project command — and equally an entry with
/// missing or unrecognized provenance — keeps native dispatch, so a duplicate claim
/// downgrades deterministically instead of depending on catalog order.
fn builtin_owned(native: &[Value], name: &str) -> bool {
    let mut builtin = false;
    for entry in native.iter().filter(|entry| matches_name(entry, name)) {
        if entry["source"] != "builtin" {
            return false;
        }
        builtin = true;
    }
    builtin
}

/// Every name an entry can claim: its own name plus its native aliases without the
/// trailing colon the native completer appends.
fn names(entry: &Value) -> impl Iterator<Item = &str> {
    std::iter::once(entry["name"].as_str().unwrap_or_default()).chain(
        entry["aliases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|alias| alias.trim_end_matches(':')),
    )
}

fn matches_name(entry: &Value, name: &str) -> bool {
    names(entry).any(|candidate| candidate == name)
}

/// Project the catalog for menus without discarding ownership. Every entry survives, because
/// a hidden name still has a native owner; `menuVisible: false` hides it while `menuAliases`
/// keeps exactly the aliases the menu may match. Hiding a builtin therefore never frees its
/// name for a client shortcut. An explicit builtin loses menu presence when its name is
/// blocked or another kind of entry claims that name; every other source (including entries
/// without provenance) keeps its real aliases and metadata-derived argument hint.
pub(super) fn catalog(mut native: Vec<Value>) -> Vec<Value> {
    for index in 0..native.len() {
        let entry = &native[index];
        let name = entry["name"].as_str().unwrap_or_default();
        let hidden =
            entry["source"] == "builtin" && (blocked_name(name) || !builtin_owned(&native, name));
        let offered = entry["aliases"].as_array().map(|aliases| {
            aliases
                .iter()
                .filter(|alias| {
                    let name = alias.as_str().unwrap_or_default().trim_end_matches(':');
                    !blockable(name, &native)
                })
                .cloned()
                .collect::<Vec<Value>>()
        });
        if hidden {
            native[index]["menuVisible"] = json!(false);
        }
        if let Some(offered) = offered {
            native[index]["menuAliases"] = json!(offered);
        }
    }
    native
}

/// Explain unavailable native behavior honestly: existing Ait controls are named only as
/// guidance (never a navigation side effect); undelivered flows say they are unavailable.
pub(super) fn rejection(name: &str) -> String {
    if TUI_ONLY.contains(&name) {
        return format!("/{name} is available only in the OMP terminal.");
    }
    if UNDELIVERED.contains(&name) {
        return format!(
            "The builtin /{name} command is not available in Ait yet. \
             Ait does not run it through the model or substitute another action."
        );
    }
    let location = match name {
        "model" | "models" | "switch" => "Ait's model selector",
        "effort" => "Ait's thinking-level control",
        "fast" | "slow" => "Ait's service-tier control",
        "mcp" | "skills" | "marketplace" | "plugins" | "plugin" | "reload-plugins" => {
            "provider integration settings"
        }
        _ => "the corresponding Ait session, workspace or provider control",
    };
    format!("Use {location}. The builtin /{name} command is disabled in Ait.")
}

/// A definitive refusal as readable timeline text; it never becomes a prompt or navigation.
pub(super) fn rejection_note(id: &str, name: &str) -> crate::protocol::timeline::NativeItem {
    let key = format!("host:omp-command-refusal:{id}");
    crate::protocol::timeline::NativeItem {
        key: key.clone(),
        turn_id: None,
        timestamp: chrono::Utc::now().to_rfc3339(),
        item: json!({"type":"assistant_message","messageId":key,"text":rejection(name)}),
    }
}

#[cfg(test)]
mod tests;
