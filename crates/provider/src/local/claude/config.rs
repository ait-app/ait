use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

/// The one extra model slot Claude Code exposes besides the main model.
///
/// The name matches OMP's role features so a unified model preset can carry it unchanged in
/// `featureValues` and the client renders it with the existing role controls.
pub(super) const ADVISOR: &str = "modelRole.advisor";
/// Follow the advisor configured by Claude's own user/project settings and flags.
pub(super) const ADVISOR_NATIVE: &str = "native";
/// Explicitly run without an advisor, whatever the user's settings say.
pub(super) const ADVISOR_UNSET: &str = "unset";

/// Advisor capability tiers from the pinned Claude Code 2.1.299 model catalog.
///
/// The CLI serves the advisor tool only for a model whose rank is at least two, and only when
/// the advisor outranks or matches the main model; aliases and custom gateway models have no
/// tier here and are left to the CLI's own check.
const ADVISOR_RANKS: &[(&str, u8)] = &[
    ("haiku-4-5", 1),
    ("sonnet-4-6", 2),
    ("opus-4-6", 3),
    ("haiku-5-5", 4),
    ("sonnet-5", 4),
    ("opus-4-7", 5),
    ("opus-4-8", 5),
    ("sonnet-5-5", 6),
    ("opus-5", 7),
    ("opus-5-5", 7),
    ("fable-5", 8),
    ("mythos-5", 8),
    ("fable-5-1", 9),
    ("mythos-5-1", 9),
];
/// Lowest tier that the CLI accepts as an advisor at all.
const MINIMUM_ADVISOR_RANK: u8 = 2;

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != "claude"
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Unavailable);
    }
    validate_shape(&spec.config)
}

/// The advisor decision for one launch, derived only from Ait's stored feature value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::local) enum Advisor<'a> {
    /// Let Claude's own user, project and local settings choose.
    Native,
    /// Refuse the advisor tool for this process, whatever those settings say.
    Disabled,
    /// Run the advisor tool on this alias or model identifier.
    Model(&'a str),
}

/// Read the stored advisor selection, defaulting to Claude's own settings.
pub(in crate::local) fn advisor(config: &StoredAgentConfig) -> Advisor<'_> {
    match config
        .feature_values
        .as_ref()
        .and_then(|values| values.get(ADVISOR))
        .and_then(Value::as_str)
    {
        None | Some(ADVISOR_NATIVE) => Advisor::Native,
        Some(ADVISOR_UNSET) => Advisor::Disabled,
        Some(model) => Advisor::Model(model),
    }
}

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    validate_shape(config)?;
    if !advisor_compatible(config) {
        return Err(AgentSessionError::Rejected);
    }
    let (fast, disabled) = model_capabilities(config.model.as_deref());
    if (config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("fast_mode"))
        == Some(&Value::Bool(true))
        && !fast)
        || (config.thinking_option_id.as_deref() == Some("off") && !disabled)
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}

/// Whether the stored advisor can serve the stored main model under the pinned catalog.
///
/// Unknown names (aliases and custom gateways) stay accepted: only the CLI knows their tier, and
/// it downgrades them to a warning instead of failing the turn. A known pair that cannot work is
/// rejected here so the client never stores a selection that silently does nothing.
fn advisor_compatible(config: &StoredAgentConfig) -> bool {
    let Advisor::Model(advisor) = advisor(config) else {
        return true;
    };
    let Some(rank) = advisor_rank(advisor) else {
        return true;
    };
    if rank < MINIMUM_ADVISOR_RANK {
        return false;
    }
    advisor_rank(config.model.as_deref().unwrap_or_default()).is_none_or(|main| rank >= main)
}

pub(super) fn validate_shape(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config.model.as_ref().is_some_and(|model| {
        model.trim().is_empty() || model.len() > 256 || model.chars().any(char::is_control)
    }) || !matches!(
        config.mode_id.as_deref(),
        None | Some("default" | "plan" | "acceptEdits" | "auto" | "bypassPermissions")
    ) || !matches!(
        config.thinking_option_id.as_deref(),
        None | Some("off" | "low" | "medium" | "high" | "xhigh" | "max" | "ultracode")
    ) || config.feature_values.as_ref().is_some_and(|values| {
        values.iter().any(|(key, value)| match key.as_str() {
            "fast_mode" => !value.is_boolean(),
            ADVISOR => !valid_advisor_selection(value),
            _ => true,
        })
    }) || config
        .system_prompt
        .as_ref()
        .is_some_and(|prompt| prompt.len() > 65536 || prompt.contains('\0'))
    {
        return Err(AgentSessionError::Rejected);
    }
    crate::local::configuration::validate(config, "claude")
}

/// A stored advisor value is an explicit keyword, an alias, or a model identifier.
///
/// Anything else is rejected because the CLI would otherwise ignore the option silently and the
/// session would run a different model set than the one the client shows.
fn valid_advisor_selection(value: &Value) -> bool {
    value.as_str().is_some_and(|selection| {
        selection == ADVISOR_NATIVE
            || selection == ADVISOR_UNSET
            || (!selection.trim().is_empty()
                && selection.len() <= 256
                && !selection.chars().any(char::is_control))
    })
}

pub(super) fn modes() -> Vec<Value> {
    [
        ("default", "Always Ask", "Shield", "safe"),
        ("plan", "Plan Mode", "ShieldEllipsis", "planning"),
        ("acceptEdits", "Accept File Edits", "ShieldPlus", "moderate"),
        ("auto", "Auto mode", "ShieldCheck", "moderate"),
        ("bypassPermissions", "Bypass", "ShieldOff", "dangerous"),
    ]
    .into_iter()
    .map(|(id, label, icon, tier)| {
        json!({"id":id,"label":label,"icon":icon,
        "colorTier":tier,"isDefault":id=="default","isUnattended":id=="bypassPermissions"})
    })
    .collect()
}

pub(super) fn features(config: &StoredAgentConfig, catalog: &[Value]) -> Vec<Value> {
    let mut features = Vec::new();
    if model_capabilities(config.model.as_deref()).0 {
        features.push(json!({"id":"fast_mode","type":"toggle","label":"Fast mode",
        "description":"Use Claude Code fast mode on a supported model",
        "value":config.feature_values.as_ref().and_then(|values|values.get("fast_mode")) == Some(&Value::Bool(true))}));
    }
    features.push(advisor_feature(config, catalog));
    features
}

/// The advisor role control. It is always offered because Claude resolves inheritance itself.
///
/// Options carry the pinned tier in `metadata.advisorRank` and, for a main model the catalog
/// knows, mark options the CLI would silently refuse so a preset author sees the limit before
/// saving. Unknown names stay selectable: only the CLI can judge a gateway or alias.
fn advisor_feature(config: &StoredAgentConfig, catalog: &[Value]) -> Value {
    let main = config
        .model
        .as_deref()
        .and_then(advisor_rank)
        .unwrap_or_default();
    let mut options = vec![
        json!({"id":ADVISOR_NATIVE,"label":"Use Claude setting","isDefault":true}),
        json!({"id":ADVISOR_UNSET,"label":"Unassigned",
            "description":"Run this session without the advisor tool"}),
    ];
    options.extend(catalog.iter().filter_map(|model| {
        let id = model["id"].as_str()?;
        let mut option = json!({"id":id,"label":model["label"].as_str().unwrap_or(id)});
        if let Some(rank) = advisor_rank(model["resolvedModel"].as_str().unwrap_or(id)) {
            option["metadata"] = json!({"advisorRank":rank});
            if rank < MINIMUM_ADVISOR_RANK || rank < main {
                option["description"] =
                    json!("Not usable as an advisor for the current main model");
            }
        }
        Some(option)
    }));
    let value = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get(ADVISOR))
        .and_then(Value::as_str)
        .unwrap_or(ADVISOR_NATIVE);
    // A saved value must stay selectable even when this catalog no longer lists it, so an agent
    // whose model was removed upstream still shows what it will run.
    if options
        .iter()
        .all(|option| option["id"].as_str() != Some(value))
    {
        options.push(json!({"id":value,"label":value}));
    }
    json!({"id":ADVISOR,"type":"select","label":"Advisor model",
        "description":"Second model that reviews the main model's work. Applies to the next turn; Claude fully switches its advisor only in a new conversation or after /compact.",
        "tooltip":"Claude Code advisor tool (next turn)","icon":"MessageCircle",
        "value":value,"options":options})
}

/// Advisor tier for a model name or alias, when the pinned catalog knows it.
fn advisor_rank(model: &str) -> Option<u8> {
    let normalized = normalize(model);
    ADVISOR_RANKS
        .iter()
        .find(|(name, _)| *name == normalized.as_str())
        .map(|(_, rank)| *rank)
}

/// Normalize a model name the way the capability table is keyed.
fn normalize(model: &str) -> String {
    let lowered = model
        .trim()
        .to_ascii_lowercase()
        .replace(['_', ' ', '.'], "-");
    let stripped = lowered
        .strip_prefix("claude-")
        .unwrap_or(&lowered)
        .replace("[1m]", "");
    stripped
        .rsplit_once('-')
        .filter(|(_, suffix)| suffix.len() == 8 && suffix.bytes().all(|v| v.is_ascii_digit()))
        .map_or_else(|| stripped.clone(), |(base, _)| base.to_owned())
}

// Capabilities follow the pinned Paseo manifest. Custom gateway aliases do not inherit
// first-party billing or thinking guarantees merely by containing a model name.
fn model_capabilities(model: Option<&str>) -> (bool, bool) {
    let normalized = normalize(model.unwrap_or_default());
    let fast = matches!(
        normalized.as_str(),
        "opus-5" | "opus-4-8" | "opus-4-7" | "opus-4-6"
    );
    (
        fast,
        fast || matches!(normalized.as_str(), "sonnet-5" | "sonnet-4-6"),
    )
}

pub(super) fn models(response: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let models = response["models"]
        .as_array()
        .filter(|models| !models.is_empty() && models.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    models
        .iter()
        .map(|model| {
            let id = text(model, "value")?;
            let label = text(model, "displayName")?;
            let efforts = model.get("supportedEffortLevels").and_then(Value::as_array);
            let mut options = efforts
                .into_iter()
                .flatten()
                .map(|effort| {
                    let id = effort.as_str().ok_or(AgentSessionError::Failed)?;
                    if !matches!(id, "low" | "medium" | "high" | "xhigh" | "max") {
                        return Err(AgentSessionError::Failed);
                    }
                    Ok(json!({"id":id,"label":id}))
                })
                .collect::<Result<Vec<_>, AgentSessionError>>()?;
            let (fast, disabled) = model_capabilities(model["resolvedModel"].as_str().or(Some(id)));
            if disabled {
                options.insert(0, json!({"id":"off","label":"Off"}));
            }
            if options.iter().any(|option| option["id"] == "xhigh") {
                options.push(json!({"id":"ultracode","label":"Ultra Code"}));
            }
            Ok(json!({"provider":"claude","id":id,"label":label,
            "description":model["description"].as_str().unwrap_or(""),"isSelectable":true,
            "isDefault":id=="default","thinkingOptions":options,"supportsFastMode":fast}))
        })
        .collect()
}

pub(super) fn commands(response: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let mut commands = response["commands"].as_array().filter(|values| values.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?.iter().map(|command| {
            Ok(json!({"name":text(command,"name")?,"description":command["description"].as_str().unwrap_or(""),
                "argumentHint":command["argumentHint"].as_str().unwrap_or("")}))
        }).collect::<Result<Vec<_>, AgentSessionError>>()?;
    if !commands.iter().any(|command| command["name"] == "rewind") {
        commands.push(json!({"name":"rewind","description":"Restore files to a previous user checkpoint","argumentHint":"[message-id]"}));
    }
    Ok(commands)
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= 1024)
        .ok_or(AgentSessionError::Failed)
}
