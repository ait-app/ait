//! Claude Code's native model-role levers as Ait feature values.
//!
//! Unlike OMP, Claude Code's role set is not provider-discovered: the harness exposes a fixed,
//! documented set of environment levers. Each assigned role becomes one `modelRole.<lever>`
//! feature value that is emitted through the `--settings` document at launch, which takes
//! precedence over the user's own `settings.json` (verified against 2.1.325).
//!
//! The main conversation model is already a first-class selection (`config.model`), so it is
//! deliberately absent here and no second control competes with it.

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use provider_core::ports::agent_session::AgentSessionError;

/// Feature-key namespace for role assignments.
pub(super) const PREFIX: &str = "modelRole.";
/// Stored value meaning "inherit the user's own Claude Code setting"; nothing is emitted.
pub(super) const INHERIT: &str = "native";
/// Stored value meaning "explicitly unset"; the lever is emitted as an empty string.
pub(super) const UNSET: &str = "unset";

const MAX_LEVER: usize = 64;
const MAX_VALUE: usize = 1024;
const MAX_ROLE_BYTES: usize = 64 * 1024;

/// One native lever and how a stored value reaches Claude Code.
struct Lever {
    /// Feature-key suffix, also the human-readable role id.
    id: &'static str,
    /// Environment variable Claude Code reads.
    variable: &'static str,
    /// Whether the value accepts an arbitrary model id or only an enumerated level.
    kind: Kind,
    /// Provider settings label.
    label: &'static str,
    /// Provider settings description.
    description: &'static str,
}

enum Kind {
    /// Any bounded model alias or full model id discovered from the native catalog.
    Model,
    /// One of Claude Code's fixed effort levels.
    Level(&'static [&'static str]),
}

const LEVELS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Every lever Ait drives, in display order.
const LEVERS: &[Lever] = &[
    Lever {
        id: "subagent",
        variable: "CLAUDE_CODE_SUBAGENT_MODEL",
        kind: Kind::Model,
        label: "Subagent model",
        description: "Model used by Task subagents",
    },
    Lever {
        id: "workflow",
        variable: "CLAUDE_CODE_WORKFLOW_SUBAGENT_MODEL",
        kind: Kind::Model,
        label: "Workflow agent model",
        description: "Model used by dynamic workflow agents",
    },
    Lever {
        id: "subagent-effort",
        variable: "CLAUDE_CODE_SUBAGENT_EFFORT",
        kind: Kind::Level(LEVELS),
        label: "Subagent effort",
        description: "Reasoning effort for subagents",
    },
    Lever {
        id: "small-fast",
        variable: "ANTHROPIC_SMALL_FAST_MODEL",
        kind: Kind::Model,
        label: "Small/fast model",
        description: "Model used for quick background tasks",
    },
    Lever {
        id: "opus",
        variable: "ANTHROPIC_DEFAULT_OPUS_MODEL",
        kind: Kind::Model,
        label: "Opus alias",
        description: "Model the opus alias resolves to",
    },
    Lever {
        id: "sonnet",
        variable: "ANTHROPIC_DEFAULT_SONNET_MODEL",
        kind: Kind::Model,
        label: "Sonnet alias",
        description: "Model the sonnet alias resolves to",
    },
    Lever {
        id: "haiku",
        variable: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        kind: Kind::Model,
        label: "Haiku alias",
        description: "Model the haiku alias resolves to",
    },
    Lever {
        id: "fable",
        variable: "ANTHROPIC_DEFAULT_FABLE_MODEL",
        kind: Kind::Model,
        label: "Fable alias",
        description: "Model the fable alias resolves to",
    },
];

/// Resolve one stored feature key to its lever.
fn lever(key: &str) -> Option<(&'static Lever, &str)> {
    key.strip_prefix(PREFIX)
        .and_then(|id| LEVERS.iter().find(|lever| lever.id == id).map(|l| (l, id)))
}

/// Whether `key` names one of Claude Code's native role levers.
///
/// A prefix-only test would accept a stale or typo'd role id that this adapter cannot emit, and
/// that key would then pass configuration validation while changing nothing at launch.
pub(super) fn is_key(key: &str) -> bool {
    lever(key).is_some()
}

fn valid_levels(kind: &Kind, value: &str) -> bool {
    match kind {
        Kind::Model => {
            !value.is_empty() && value.len() <= MAX_VALUE && !value.chars().any(char::is_control)
        }
        Kind::Level(levels) => levels.contains(&value),
    }
}

/// Validate one role value's bounded shape; enumerated levels are checked against `kind`.
fn valid_value(kind: &Kind, value: &str) -> bool {
    matches!(value, INHERIT | UNSET) || valid_levels(kind, value)
}

/// Validate one role feature's bounded shape before any catalog lookup.
pub(super) fn valid(key: &str, value: &Value) -> bool {
    let Some((lever, _)) = lever(key) else {
        return false;
    };
    value
        .as_str()
        .is_some_and(|value| valid_value(&lever.kind, value))
}

/// The explicit environment overrides a stored configuration produces.
///
/// Inherited roles emit nothing, so the user's own Claude Code setting stays in force.
/// An unset role emits an empty value, which is how Claude Code turns a lever off.
pub(super) fn environment(config: &StoredAgentConfig) -> Vec<(&'static str, String)> {
    config
        .feature_values
        .iter()
        .flatten()
        .filter_map(|(key, value)| {
            let (lever, _) = lever(key)?;
            let value = value.as_str()?.trim();
            match value {
                INHERIT | "" => None,
                UNSET => Some((lever.variable, String::new())),
                value => Some((lever.variable, value.to_owned())),
            }
        })
        .collect()
}

/// Render role controls for a provider settings listing.
///
/// Every model kind offers the discovered native model ids plus explicit inherit/unset rows.
/// The discovered list is already the resolved catalog, so an alias such as `opus` is a valid
/// assignment even when its target is a custom gateway model.
pub(super) fn features(config: &StoredAgentConfig, models: &[Value]) -> Vec<Value> {
    let choices: Vec<(&str, String)> = models
        .iter()
        .filter_map(|model| {
            let id = model["id"].as_str()?;
            let label = model["label"].as_str().unwrap_or(id);
            Some((id, format!("{label} · {id}")))
        })
        .collect();
    LEVERS
        .iter()
        .map(|lever| {
            let key = format!("{PREFIX}{}", lever.id);
            let stored = config
                .feature_values
                .as_ref()
                .and_then(|values| values.get(&key))
                .and_then(Value::as_str)
                .unwrap_or(INHERIT);
            let mut options =
                vec![json!({"id":INHERIT,"label":"Use Claude Code setting","isDefault":true})];
            options.push(json!({"id":UNSET,"label":"Unassigned"}));
            match &lever.kind {
                Kind::Model => {
                    options.extend(
                        choices
                            .iter()
                            .map(|(id, label)| json!({"id":id,"label":label})),
                    );
                }
                Kind::Level(levels) => {
                    options.extend(levels.iter().map(|level| json!({"id":level,"label":level})));
                }
            }
            // Keep an unknown stored value selectable instead of silently rerouting the role.
            if !options
                .iter()
                .any(|option| option["id"].as_str() == Some(stored))
            {
                options.push(json!({"id":stored,"label":stored,"metadata":{"unresolved":true}}));
            }
            json!({"id":key,"type":"select","label":lever.label,
                "description":lever.description,"value":stored,"options":options})
        })
        .collect()
}

/// Whether a configuration's role assignments fit the bounded feature budget.
///
/// # Errors
/// Returns [`AgentSessionError::Rejected`] when the assignment set is oversized or malformed.
pub(super) fn check_size(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    // Measure the encoded assignments, not only what would be emitted: an inherited per-lever
    // entry is real stored state and must stay within the same budget.
    let roles: Vec<(&str, &str)> = config
        .feature_values
        .iter()
        .flatten()
        .filter_map(|(key, value)| {
            let (_, id) = lever(key)?;
            Some((id, value.as_str()?))
        })
        .collect();
    let encoded = serde_json::to_vec(&roles).map_err(|_| AgentSessionError::Rejected)?;
    if encoded.len() > MAX_ROLE_BYTES
        || roles
            .iter()
            .any(|(id, value)| id.len() > MAX_LEVER || value.len() > MAX_VALUE)
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
