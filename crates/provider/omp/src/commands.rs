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

/// How Ait answers a slash command whose native entry it blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Managed {
    /// Ait has a control that changes the setting; the answer names it.
    Surface(&'static str),
    /// Ait owns the setting but ships no control for it yet, so the answer must not send the
    /// user looking for one.
    NoControl,
    /// Ait does not deliver the ability yet; the native handler must not run behind its back.
    Unavailable,
    /// A terminal-only OMP command that Ait never offers and never displaces with its own UI.
    TerminalOnly,
}

/// Slash commands Ait blocks from OMP: every row of `docs/plans/pr612-split/sources.json`'s
/// `commands_tsv` whose entry strategy blocks the native slash entry, one entry per name
/// including aliases. Blocked text is refused in the input box, left out of completion, and
/// answered in the timeline; it never reaches OMP, so a native handler cannot change a
/// GUI-owned setting or run a capability Ait has not taken over.
const MANAGED: [(&str, Managed); 69] = [
    // Native entry replaced by an Ait control.
    ("model", Managed::Surface("the model selector")),
    ("models", Managed::Surface("the model selector")),
    ("switch", Managed::Surface("the model selector")),
    (
        "modelpreset",
        Managed::Surface("Settings › Providers › Model presets"),
    ),
    ("effort", Managed::Surface("the Thinking control")),
    ("fast", Managed::Surface("the Speed control")),
    ("settings", Managed::Surface("Settings")),
    ("setup", Managed::Surface("Settings › Providers")),
    ("providers", Managed::Surface("Settings › Providers")),
    ("login", Managed::Surface("the Online Service account page")),
    (
        "logout",
        Managed::Surface("the Online Service account page"),
    ),
    ("hub", Managed::Surface("the Subagents panel")),
    ("subagents", Managed::Surface("the Subagents panel")),
    ("resume", Managed::Surface("History")),
    ("branch", Managed::Surface("the Rewind menu")),
    ("rewind", Managed::Surface("the Rewind menu")),
    // Settings the GUI owns with no control of its own yet.
    ("advisor", Managed::NoControl),
    ("prewalk", Managed::NoControl),
    ("slow", Managed::NoControl),
    ("skillful", Managed::NoControl),
    ("extended-context", Managed::NoControl),
    ("computer", Managed::NoControl),
    ("browser", Managed::NoControl),
    ("agents", Managed::NoControl),
    ("tree", Managed::NoControl),
    ("export", Managed::NoControl),
    ("dump", Managed::NoControl),
    // Entries Ait has not taken over: discovered, but submission is refused.
    ("todo", Managed::Unavailable),
    ("plan-review", Managed::Unavailable),
    ("new", Managed::Unavailable),
    ("clear", Managed::Unavailable),
    ("delete", Managed::Unavailable),
    ("fresh", Managed::Unavailable),
    ("shake", Managed::Unavailable),
    ("pin", Managed::Unavailable),
    ("unpin", Managed::Unavailable),
    ("rename", Managed::Unavailable),
    ("move", Managed::Unavailable),
    ("wt", Managed::Unavailable),
    ("worktree", Managed::Unavailable),
    ("fork", Managed::Unavailable),
    ("restart", Managed::Unavailable),
    ("mcp", Managed::Unavailable),
    ("skills", Managed::Unavailable),
    ("marketplace", Managed::Unavailable),
    ("plugins", Managed::Unavailable),
    ("plugin", Managed::Unavailable),
    ("reload-plugins", Managed::Unavailable),
    ("ssh", Managed::Unavailable),
    ("forget-approvals", Managed::Unavailable),
    ("add-dir", Managed::Unavailable),
    ("remove-dir", Managed::Unavailable),
    ("copy", Managed::Unavailable),
    ("open", Managed::Unavailable),
    ("local", Managed::Unavailable),
    ("git", Managed::Unavailable),
    ("share", Managed::Unavailable),
    ("collab", Managed::Unavailable),
    ("join", Managed::Unavailable),
    ("leave", Managed::Unavailable),
    ("pause", Managed::Unavailable),
    ("live", Managed::Unavailable),
    ("retry", Managed::Unavailable),
    // Terminal-only commands with no Ait replacement.
    ("hotkeys", Managed::TerminalOnly),
    ("quit", Managed::TerminalOnly),
    ("q", Managed::TerminalOnly),
    ("exit", Managed::TerminalOnly),
    ("record", Managed::TerminalOnly),
    ("debug", Managed::TerminalOnly),
];

/// Most commands accepted from one OMP command list.
const MAX_COMMANDS: usize = 4096;
/// Bound aggregate aliases independently of the number of catalog entries.
const MAX_ALIASES: usize = 4096;

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

/// The Ait ownership of `text`'s command when Ait blocks its native entry, or `None` for any
/// other text. Names match case-insensitively; arguments such as `/advisor off` do not
/// change the match.
///
/// # Returns
///
/// The command's lowercased name and how Ait answers it.
pub(super) fn gui_managed(text: &str) -> Option<(&'static str, Managed)> {
    let (name, _) = split(text)?;
    let name = name.to_ascii_lowercase();
    MANAGED
        .iter()
        .find(|(command, _)| *command == name)
        .copied()
}

/// The answer shown when `text` invokes a command Ait blocks: the command is not run, and the
/// user learns where the setting lives or that Ait has no replacement yet.
///
/// # Arguments
///
/// * `name` - The lowercased command name, without its slash.
/// * `managed` - How Ait owns the command's native entry.
///
/// # Returns
///
/// The card label text for the refused command.
pub(super) fn gui_managed_note(name: &str, managed: Managed) -> String {
    match managed {
        Managed::Surface(surface) => {
            format!("`/{name}` is managed by Ait. Change it in {surface}.")
        }
        Managed::NoControl => {
            format!("`/{name}` is managed by Ait, which has no control for it yet.")
        }
        Managed::Unavailable => {
            format!("`/{name}` is not available in Ait yet, so it is not sent to OMP.")
        }
        Managed::TerminalOnly => {
            format!("`/{name}` is a terminal-only OMP command, which Ait does not run.")
        }
    }
}

/// The name of the slash command `text` invokes, lowercased, for labelling the cards of its
/// answer. Any slash command qualifies, including the ones OMP handles itself such as
/// `/session`; text that is no slash command has no name.
pub(super) fn slash_name(text: &str) -> Option<String> {
    Some(split(text)?.0.to_ascii_lowercase())
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

/// A native catalog and the ownership of each name and alias. A collision or missing
/// provenance is never classified as builtin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Catalog {
    entries: Vec<Value>,
    claims: HashMap<String, Claim>,
}

/// A source conflict or ambiguous builtin alias cannot be dispatched safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Claim {
    Builtin(String),
    Native,
    Unknown(String),
    Ambiguous,
}

impl Catalog {
    /// Decode the complete native catalog before using it for either display or routing.
    pub(super) fn parse(value: &Value) -> Result<Self, AgentSessionError> {
        let entries = value["commands"]
            .as_array()
            .filter(|commands| commands.len() <= MAX_COMMANDS)
            .ok_or(AgentSessionError::Failed)?;
        let mut claims = HashMap::new();
        let mut alias_count = 0_usize;
        for entry in entries {
            let name = config::text(entry, "name")?;
            let claim = match entry["source"].as_str() {
                Some("builtin") => Claim::Builtin(name.to_owned()),
                Some(
                    "extension" | "skill" | "custom" | "file" | "project" | "template" | "prompt",
                ) => Claim::Native,
                _ => Claim::Unknown(name.to_owned()),
            };
            let aliases = aliases(entry)?;
            alias_count = alias_count.saturating_add(aliases.len());
            if alias_count > MAX_ALIASES {
                return Err(AgentSessionError::Failed);
            }
            for name in std::iter::once(name).chain(aliases) {
                let key = name.trim_end_matches(':').to_ascii_lowercase();
                claims
                    .entry(key)
                    .and_modify(|previous| {
                        *previous = match (&*previous, &claim) {
                            (Claim::Native, Claim::Native) => Claim::Native,
                            (Claim::Unknown(name), Claim::Native)
                            | (Claim::Native, Claim::Unknown(name)) => Claim::Unknown(name.clone()),
                            (Claim::Unknown(first), Claim::Unknown(second))
                                if first.eq_ignore_ascii_case(second) =>
                            {
                                previous.clone()
                            }
                            (Claim::Builtin(first), Claim::Builtin(second))
                                if first.eq_ignore_ascii_case(second) =>
                            {
                                previous.clone()
                            }
                            _ => Claim::Ambiguous,
                        };
                    })
                    .or_insert_with(|| claim.clone());
            }
        }
        Ok(Self {
            entries: entries.clone(),
            claims,
        })
    }

    /// Whether at least one native command claims this name or alias.
    pub(super) fn contains(&self, name: &str) -> bool {
        self.claims
            .contains_key(&name.trim_end_matches(':').to_ascii_lowercase())
    }

    /// Only explicit builtin claims count; conflicting or absent sources do not.
    pub(super) fn builtin_owned(&self, name: &str) -> bool {
        matches!(self.claim(name), Some(Claim::Builtin(_)))
    }

    /// Return the unambiguous canonical owner for a name or alias.
    pub(super) fn claim(&self, name: &str) -> Option<&Claim> {
        self.claims
            .get(&name.trim_end_matches(':').to_ascii_lowercase())
    }

    pub(super) fn project(&self) -> Vec<Value> {
        let mut output = Vec::with_capacity(BUILTINS.len() + self.entries.len());
        for (name, description, hint) in BUILTINS {
            if !self.contains(name) {
                output.push(json!({"name":name,"description":description,"argumentHint":hint,"kind":"command"}));
            }
        }
        for entry in &self.entries {
            let name = entry["name"].as_str().expect("validated command name");
            let hint = entry["input"]["hint"]
                .as_str()
                .or_else(|| {
                    BUILTINS
                        .iter()
                        .find(|(builtin, _, _)| builtin.eq_ignore_ascii_case(name))
                        .map(|(_, _, hint)| *hint)
                })
                .unwrap_or_default();
            let description = entry["description"]
                .as_str()
                .or_else(|| entry["source"].as_str())
                .unwrap_or("command");
            let mut command = json!({"name":name,"description":description,"argumentHint":hint,
                "kind":if entry["source"] == "skill" {"skill"} else {"command"}});
            if let Some(source) = entry["source"].as_str() {
                command["source"] = json!(source);
            }
            if matches!(self.claim(name), Some(Claim::Ambiguous))
                || matches!(self.claim(name), Some(Claim::Unknown(canonical)) if host_reserved(name) || host_reserved(canonical))
                || (entry["source"] == "builtin"
                    && (gui_managed(&format!("/{name}")).is_some() || !self.builtin_owned(name)))
            {
                command["menuVisible"] = json!(false);
            }
            if let Some(raw) = entry["aliases"].as_array() {
                command["aliases"] = json!(raw);
                let visible: Vec<_> = raw
                    .iter()
                    .filter(|alias| {
                        let name = alias
                            .as_str()
                            .expect("validated alias")
                            .trim_end_matches(':');
                        !matches!(self.claim(name), Some(Claim::Ambiguous))
                            && !matches!(self.claim(name), Some(Claim::Unknown(canonical)) if host_reserved(name) || host_reserved(canonical))
                            && (entry["source"] != "builtin"
                                || (self.builtin_owned(name)
                                    && gui_managed(&format!(
                                        "/{}",
                                        entry["name"].as_str().unwrap_or_default()
                                    ))
                                    .is_none()
                                    && gui_managed(&format!("/{name}")).is_none()))
                    })
                    .cloned()
                    .collect();
                command["menuAliases"] = json!(visible);
            }
            output.push(command);
        }
        output
    }
}

fn host_reserved(name: &str) -> bool {
    gui_managed(&format!("/{name}")).is_some() || builtin(&format!("/{name} argument")).is_some()
}

fn aliases(entry: &Value) -> Result<impl ExactSizeIterator<Item = &str>, AgentSessionError> {
    let values = match entry.get("aliases") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(values)) if values.len() <= MAX_COMMANDS => values.as_slice(),
        Some(_) => return Err(AgentSessionError::Failed),
    };
    if values.iter().any(|value| {
        value.as_str().is_none_or(|text| {
            text.trim_end_matches(':').is_empty()
                || text.len() > 4096
                || text.chars().any(char::is_control)
        })
    }) {
        return Err(AgentSessionError::Failed);
    }
    Ok(values
        .iter()
        .map(|value| value.as_str().expect("validated alias")))
}

/// Project a complete OMP catalog without losing source, aliases, or hidden ownership.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a missing or oversized list or an invalid name.
pub(super) fn list(value: &Value) -> Result<Vec<Value>, AgentSessionError> {
    Ok(Catalog::parse(value)?.project())
}

#[cfg(test)]
mod tests;
