//! OMP slash commands: the built-ins Ait runs beside foreground input (M5), as Paseo's
//! `tryHandleOutOfBand` does, and the command list merged with OMP's own (D25).

use std::collections::HashMap;

use serde_json::{Value, json};

use super::config;
use provider_core::ports::agent_session::AgentSessionError;

/// Built-in commands Ait handles out of band: name, description and argument hint, as
/// Paseo's `OMP_HANDLED_BUILTIN_SLASH_COMMANDS`.
const BUILTINS: [(&str, &str, &str); 5] = [
    (
        "compact",
        "Manually compact the session context",
        "[instructions]",
    ),
    (
        "autocompact",
        "Toggle automatic context compaction",
        "[on|off|toggle]",
    ),
    (
        "handoff",
        "Hand off from planning to implementation",
        "[instructions]",
    ),
    ("steer", "Steer the active OMP turn", "<message>"),
    (
        "follow-up",
        "Queue a follow-up message for OMP",
        "<message>",
    ),
];

/// Most commands accepted from one OMP command list.
const MAX_COMMANDS: usize = 4096;

/// A built-in command Ait runs out of band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Builtin {
    /// `/compact [instructions]`.
    Compact,
    /// `/autocompact [on|off|toggle]`.
    AutoCompact,
    /// `/handoff [instructions]`.
    Handoff,
    /// `/steer <message>`: a `prompt` with OMP's `steer` streaming behaviour.
    Steer,
    /// `/follow-up <message>`: a `prompt` with OMP's `followUp` streaming behaviour.
    FollowUp,
}

/// What `/autocompact` asks for, as Paseo's `parseAutoCompactMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AutoCompact {
    /// Enable (`true`) or disable automatic compaction.
    Set(bool),
    /// Flip OMP's current `autoCompactionEnabled`.
    Toggle,
    /// An argument Paseo does not accept.
    Unknown,
}

/// Split slash text into its command name and trimmed arguments, as Paseo's
/// `parseSlashCommandInput`: `/name` with optional arguments, where the name contains no `/`.
pub(super) fn split(text: &str) -> Option<(&str, &str)> {
    let command = text.trim().strip_prefix('/')?;
    let (name, args) = command
        .split_once(char::is_whitespace)
        .unwrap_or((command, ""));
    (!name.is_empty() && !name.contains('/')).then_some((name, args.trim()))
}

/// The built-in `text` invokes, with its arguments. `/steer` and `/follow-up` without a
/// message are ordinary prompts, as in Paseo; names match case-insensitively.
pub(super) fn builtin(text: &str) -> Option<(Builtin, &str)> {
    let (name, args) = split(text)?;
    let command = match name.to_ascii_lowercase().as_str() {
        "compact" => Builtin::Compact,
        "autocompact" => Builtin::AutoCompact,
        "handoff" => Builtin::Handoff,
        "steer" if !args.is_empty() => Builtin::Steer,
        "follow-up" if !args.is_empty() => Builtin::FollowUp,
        _ => return None,
    };
    Some((command, args))
}

/// Parse `/autocompact`'s argument; none toggles.
pub(super) fn auto_compact(args: &str) -> AutoCompact {
    match args.to_ascii_lowercase().as_str() {
        "on" | "true" | "enable" | "enabled" => AutoCompact::Set(true),
        "off" | "false" | "disable" | "disabled" => AutoCompact::Set(false),
        "" | "toggle" => AutoCompact::Toggle,
        _ => AutoCompact::Unknown,
    }
}

/// Map OMP's `{commands}` (from `get_available_commands`) into Ait commands, as Paseo's
/// `mapOmpSlashCommands`: the built-ins first, then OMP's commands, which replace a built-in
/// of the same name but keep its argument hint when they have none. A description falls back
/// to the command's `source`, then `"command"`; `skill` sources are skills.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing or oversized list or an invalid name.
pub(super) fn list(value: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let native = value["commands"]
        .as_array()
        .filter(|commands| commands.len() <= MAX_COMMANDS)
        .ok_or(AgentSessionError::Failed)?;
    let mut commands = Vec::with_capacity(BUILTINS.len() + native.len());
    // Position of each listed name, so a later entry replaces an earlier one in place.
    let mut positions = HashMap::with_capacity(commands.capacity());
    for (name, description, hint) in BUILTINS {
        positions.insert(name, commands.len());
        commands.push(
            json!({"name":name,"description":description,"argumentHint":hint,"kind":"command"}),
        );
    }
    for entry in native {
        let name = config::text(entry, "name")?;
        let known = positions.get(name).copied();
        let hint = entry["input"]["hint"]
            .as_str()
            .or_else(|| known.and_then(|index| commands[index]["argumentHint"].as_str()))
            .unwrap_or_default()
            .to_owned();
        let description = entry["description"]
            .as_str()
            .or_else(|| entry["source"].as_str())
            .unwrap_or("command");
        let command = json!({"name":name,"description":description,"argumentHint":hint,
            "kind":if entry["source"] == "skill" {"skill"} else {"command"}});
        if let Some(index) = known {
            commands[index] = command;
        } else {
            positions.insert(name, commands.len());
            commands.push(command);
        }
    }
    Ok(commands)
}

#[cfg(test)]
mod tests;
