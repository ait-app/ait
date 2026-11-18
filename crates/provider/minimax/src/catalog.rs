//! Separate model and thinking selection over `MiniMax` Code's combined model catalog.
//!
//! Native ACP lists each model once per reasoning variant (`m:minimax:MiniMax-M3:v:thinking`)
//! and exposes an effort select only for some variants. AIT shows the model once and moves
//! the variant and effort into its thinking options: `off` selects the empty variant, a bare
//! variant (`thinking`) selects it without effort control, and `<variant>:<effort>` selects
//! both. Values without a variant marker keep native effort identities unchanged.

use std::collections::HashMap;

use serde_json::{Value, json};

use provider_core::ports::agent_session::AgentSessionError;
use provider_native::acp_config::{choices, text};

use super::PROVIDER;

/// Native separator between a model and its reasoning variant.
const VARIANT: &str = ":v:";
/// Thinking identity of the native empty (non-reasoning) variant.
const OFF: &str = "off";
/// Separator between a model's name and its variant in native labels.
const LABEL_SEPARATOR: &str = " \u{b7} ";

/// One advertised native model choice and what selecting it in discovery confirmed.
#[derive(Debug)]
pub(super) struct Probe {
    /// Native choice as advertised before probing.
    pub(super) choice: Value,
    /// Whether the native agent accepted the choice.
    pub(super) accepted: bool,
    /// Native effort select confirmed for the choice, when it offers one.
    pub(super) efforts: Option<Value>,
}

impl Probe {
    fn value(&self) -> &str {
        self.choice["value"].as_str().unwrap_or_default()
    }
}

/// Native model choices to try, in preference order, plus the effort to apply afterwards.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Selection {
    /// Candidate native model values; empty keeps the current model.
    pub(super) models: Vec<String>,
    /// Native effort value to select after the model.
    pub(super) effort: Option<String>,
}

/// Split a native model value into the AIT model identity and its reasoning variant.
pub(super) fn split(value: &str) -> (&str, Option<&str>) {
    value
        .rsplit_once(VARIANT)
        .map_or((value, None), |(model, variant)| (model, Some(variant)))
}

/// Thinking identity naming a reasoning `variant`.
fn token(variant: &str) -> &str {
    if variant.is_empty() { OFF } else { variant }
}

/// Encode a native `variant` and `effort` as one AIT thinking identity; `None` when neither
/// selects anything.
pub(super) fn thinking_id(variant: Option<&str>, effort: Option<&str>) -> Option<String> {
    match (variant, effort) {
        (None, effort) => effort.map(str::to_owned),
        (Some(variant), None) => Some(token(variant).to_owned()),
        (Some(variant), Some(effort)) => Some(format!("{}:{effort}", token(variant))),
    }
}

/// Native label for a `variant` of a choice called `name`, such as `Thinking` for
/// `M3 · thinking`.
fn variant_label(variant: &str, name: &str) -> String {
    if variant.is_empty() {
        return "Off".to_owned();
    }
    let label = name
        .rsplit_once(LABEL_SEPARATOR)
        .map_or(variant, |(_, suffix)| suffix);
    let mut characters = label.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// Resolve a host `model` and `thinking` choice against the native model select `native`.
///
/// A complete native value is passed through with `thinking` as its effort, so configurations
/// stored before the split keep working. Without `thinking`, every variant of the model is a
/// candidate, preferring the current value and then its variant, so a variant the native agent
/// rejects falls back to the next one.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for an unknown model or thinking identity, and
/// `AgentSessionError::Failed` for a malformed native catalog.
pub(super) fn resolve(
    native: &Value,
    model: Option<&str>,
    thinking: Option<&str>,
) -> Result<Selection, AgentSessionError> {
    let values = choices(native)?
        .into_iter()
        .map(|choice| text(choice, "value"))
        .collect::<Result<Vec<_>, _>>()?;
    let effort = thinking.map(str::to_owned);
    if let Some(model) = model.filter(|model| values.contains(model)) {
        return Ok(Selection {
            models: vec![model.to_owned()],
            effort,
        });
    }
    if model.is_none() && thinking.is_none() {
        return Ok(Selection {
            models: Vec::new(),
            effort: None,
        });
    }
    let current = text(native, "currentValue")?;
    let (current_model, current_variant) = split(current);
    let model = model.unwrap_or(current_model);
    let mut members = values
        .into_iter()
        .filter(|value| split(value).0 == model)
        .collect::<Vec<_>>();
    if members.is_empty() {
        return Err(AgentSessionError::Rejected);
    }
    let Some(thinking) = thinking else {
        members.sort_by_key(|value| (*value != current, split(value).1 != current_variant));
        return Ok(Selection {
            models: members.into_iter().map(str::to_owned).collect(),
            effort: None,
        });
    };
    let variant = |name: &str| {
        members
            .iter()
            .find(|value| split(value).1.is_some_and(|variant| token(variant) == name))
    };
    let (value, effort) = if let Some(value) = variant(thinking) {
        (value, None)
    } else if let Some((value, effort)) = thinking
        .rsplit_once(':')
        .and_then(|(name, effort)| variant(name).map(|value| (value, effort)))
    {
        (value, Some(effort.to_owned()))
    } else {
        let value = members
            .iter()
            .find(|value| split(value).1.is_none())
            .ok_or(AgentSessionError::Rejected)?;
        (value, effort)
    };
    Ok(Selection {
        models: vec![(*value).to_owned()],
        effort,
    })
}

/// Project probed native choices as one model per identity with variant-aware thinking options.
///
/// `current` is the native default value before probing. A model is selectable when the native
/// agent accepted at least one of its variants; rejected variants offer no thinking option.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for malformed native choices or effort selects.
pub(super) fn models(current: &str, probes: &[Probe]) -> Result<Vec<Value>, AgentSessionError> {
    let mut groups: Vec<(&str, Vec<&Probe>)> = Vec::new();
    let mut index = HashMap::with_capacity(probes.len());
    for probe in probes {
        let model = split(probe.value()).0;
        let position = *index.entry(model).or_insert_with(|| {
            groups.push((model, Vec::new()));
            groups.len() - 1
        });
        groups[position].1.push(probe);
    }
    groups
        .into_iter()
        .map(|(model, members)| project(model, &members, current))
        .collect()
}

/// Project one model identity from its native `members`, given the native `current` value.
fn project(model: &str, members: &[&Probe], current: &str) -> Result<Value, AgentSessionError> {
    let first = members.first().ok_or(AgentSessionError::Failed)?;
    let name = text(&first.choice, "name")?;
    let label = match split(first.value()).1 {
        Some(variant) if !variant.is_empty() => name
            .rsplit_once(LABEL_SEPARATOR)
            .map_or(name, |(model, _)| model),
        _ => name,
    };
    let accepted = members
        .iter()
        .filter(|probe| probe.accepted)
        .collect::<Vec<_>>();
    let current_variant = split(current).1;
    let preferred = accepted
        .iter()
        .find(|probe| probe.value() == current)
        .or_else(|| {
            accepted
                .iter()
                .find(|probe| split(probe.value()).1 == current_variant)
        })
        .or_else(|| accepted.first());
    let default = preferred.and_then(|probe| {
        let effort = probe
            .efforts
            .as_ref()
            .and_then(|efforts| efforts["currentValue"].as_str());
        thinking_id(split(probe.value()).1, effort)
    });
    let prefixed = accepted
        .iter()
        .filter(|probe| probe.efforts.is_some())
        .count()
        > 1;
    let mut options = Vec::new();
    for probe in &accepted {
        options.extend(thinking_options(probe, prefixed)?);
    }
    for option in &mut options {
        option["isDefault"] = json!(default.as_deref() == option["id"].as_str());
    }
    let mut projected = json!({"provider":PROVIDER,"id":model,"label":label,
        "isDefault":accepted.iter().any(|probe| probe.value() == current),
        "isSelectable":!accepted.is_empty(),"thinkingOptions":options});
    if let Some(description) = first.choice["description"].as_str() {
        projected["description"] = json!(description);
    }
    if let Some(default) = default {
        projected["defaultThinkingOptionId"] = json!(default);
    }
    Ok(projected)
}

/// Thinking options offered by one accepted native variant; `prefixed` names the variant in
/// effort labels when several variants of the model offer efforts.
fn thinking_options(probe: &Probe, prefixed: bool) -> Result<Vec<Value>, AgentSessionError> {
    let variant = split(probe.value()).1;
    let name = text(&probe.choice, "name")?;
    let Some(efforts) = &probe.efforts else {
        return Ok(variant
            .map(|variant| vec![json!({"id":token(variant),"label":variant_label(variant, name)})])
            .unwrap_or_default());
    };
    choices(efforts)?
        .into_iter()
        .map(|choice| {
            let effort = text(choice, "name")?;
            let label = match variant {
                Some(variant) if prefixed => {
                    format!("{}{LABEL_SEPARATOR}{effort}", variant_label(variant, name))
                }
                _ => effort.to_owned(),
            };
            Ok(json!({"id":thinking_id(variant, choice["value"].as_str()),"label":label}))
        })
        .collect()
}
