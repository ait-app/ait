//! Native presets become private Ait selections, never global role mutations.

pub(super) mod management;
mod resolution;

use std::collections::BTreeMap;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::super::OmpClient;
use super::{Catalog, PREFIX, Role, feature, is_key, valid_role};
use crate::ports::agent_session::AgentSessionError;

#[derive(Debug, Clone, Default)]
/// Bounded native definitions and acknowledged private configuration snapshots.
pub(super) struct Presets {
    entries: BTreeMap<String, Entry>,
    basis: Value,
}

#[derive(Debug, Clone)]
struct Entry {
    raw: Value,
    source: &'static str,
    configuration: Result<StoredAgentConfig, String>,
}

struct Context<'a> {
    catalog: &'a Catalog,
    settings: &'a Value,
    previous: Option<&'a Catalog>,
}

/// Names follow OMP's public model-preset naming rule, with a transport size bound.
pub(in super::super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Read each owning layer whole: same-name project presets must not deep-merge global entries.
pub(super) async fn read(
    client: &OmpClient,
    cwd: &str,
    settings: &Value,
    catalog: &Catalog,
) -> Result<Presets, AgentSessionError> {
    if settings["modelPresets"]["value"].is_null()
        || settings["modelPresets"]["value"]
            .as_object()
            .is_some_and(serde_json::Map::is_empty)
    {
        return Ok(Presets::default());
    }
    let global = super::read(
        client,
        cwd,
        &["config", "list", "--scope", "global", "--json"],
    )
    .await?;
    let project = super::read(
        client,
        cwd,
        &["config", "list", "--scope", "project", "--json"],
    )
    .await?;
    let mut owned = BTreeMap::new();
    for (layer, source) in [(&global, "global"), (&project, "project")] {
        if let Some(entries) = layer["modelPresets"]["value"].as_object() {
            for (name, raw) in entries {
                if valid_name(name) && !raw.is_null() {
                    owned.insert(name.clone(), (sanitize(raw), source));
                }
            }
        } else if !layer["modelPresets"]["value"].is_null() {
            return Err(AgentSessionError::Failed);
        }
    }
    if owned.len() > 128 {
        return Err(AgentSessionError::Failed);
    }
    let previous = client.cached_roles(cwd)?;
    let basis = json!({"models":catalog.models,"roles":catalog.inherited,"thinking":settings["defaultThinkingLevel"]["value"]});
    let previous = previous.as_ref().filter(|old| old.presets.basis == basis);
    let mut presets = Presets {
        entries: BTreeMap::new(),
        basis,
    };
    let context = Context {
        catalog,
        settings,
        previous: previous.map(AsRef::as_ref),
    };
    let mut pending = owned.into_iter();
    while let Some(first) = pending.next() {
        let second = pending.next();
        let (first, second) = tokio::join!(load_entry(client, cwd, &context, first), async {
            match second {
                Some(second) => Some(load_entry(client, cwd, &context, second).await),
                None => None,
            }
        });
        presets.entries.insert(first.0, first.1);
        if let Some((name, entry)) = second {
            presets.entries.insert(name, entry);
        }
    }
    Ok(presets)
}

async fn load_entry(
    client: &OmpClient,
    cwd: &str,
    context: &Context<'_>,
    input: (String, (Value, &'static str)),
) -> (String, Entry) {
    let (name, (raw, source)) = input;
    let cached = context
        .previous
        .and_then(|old| old.presets.entries.get(&name))
        .filter(|entry| entry.raw == raw && entry.source == source && entry.configuration.is_ok());
    let configuration = match cached {
        Some(entry) => entry.configuration.clone(),
        None => materialize(client, cwd, context.catalog, context.settings, &raw).await,
    };
    (
        name,
        Entry {
            raw,
            source,
            configuration,
        },
    )
}

fn parse_roles(raw: &Value) -> Result<BTreeMap<String, String>, String> {
    let roles: BTreeMap<String, String> = serde_json::from_value(raw["modelRoles"].clone())
        .map_err(|_| "Preset modelRoles must be a mapping of selectors".to_owned())?;
    if roles.len() > 128
        || roles.iter().any(|(role, selector)| {
            !valid_role(role)
                || selector.is_empty()
                || selector.len() > 2048
                || selector.chars().any(char::is_control)
        })
    {
        return Err("Preset contains invalid role assignments".to_owned());
    }
    Ok(roles)
}

fn sanitize(raw: &Value) -> Value {
    let Ok(roles) = parse_roles(raw) else {
        return Value::Null;
    };
    let mut value = json!({"modelRoles":roles});
    if let Some(thinking) = raw.get("defaultThinkingLevel") {
        if !thinking.as_str().is_some_and(valid_thinking) {
            return Value::Null;
        }
        value["defaultThinkingLevel"] = thinking.clone();
    }
    value
}

/// Whether the preset contains a native default thinking level.
pub(super) fn valid_thinking(value: &str) -> bool {
    matches!(
        value,
        "auto" | "off" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

async fn materialize(
    client: &OmpClient,
    cwd: &str,
    catalog: &Catalog,
    settings: &Value,
    raw: &Value,
) -> Result<StoredAgentConfig, String> {
    let roles = parse_roles(raw)?;
    let thinking = raw
        .get("defaultThinkingLevel")
        .unwrap_or(&settings["defaultThinkingLevel"]["value"]);
    let thinking = if thinking.is_null() {
        "auto"
    } else {
        thinking
            .as_str()
            .filter(|level| valid_thinking(level))
            .ok_or("Preset thinking level is invalid")?
    };
    let mut values: BTreeMap<_, _> = catalog
        .roles
        .iter()
        .map(|role| (format!("{PREFIX}{}", role.id), json!("unset")))
        .collect();
    values.extend(
        roles
            .iter()
            .map(|(role, selector)| (format!("{PREFIX}{role}"), json!(selector))),
    );
    values.insert("modelPresetThinking".to_owned(), json!(thinking));
    let snapshot = serde_json::to_string(&roles).map_err(|_| "Invalid preset roles")?;
    if snapshot.len() > 65536 {
        return Err("Preset roles exceed the configuration budget".to_owned());
    }
    values.insert("modelPresetRoles".to_owned(), json!(snapshot));
    values.insert("modelPreset".to_owned(), Value::Null);
    let mut config = StoredAgentConfig {
        feature_values: Some(values),
        ..Default::default()
    };
    // Native startup resolves aliases, fuzzy selectors, authentication and automatic defaults.
    // The probe has no session and never submits a prompt or writes role settings.
    let state = resolution::read(client, cwd, &config, roles.contains_key("default")).await?;
    let runtime = super::super::config::runtime(&state)
        .map_err(|_| "OMP returned an invalid preset model".to_owned())?;
    config.model = runtime.model;
    config.thinking_option_id = runtime.thinking_option_id;
    Ok(config)
}

impl Presets {
    /// Whether this directory defines any named presets.
    pub(super) fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Whether a preset introduces this native role.
    pub(super) fn has_role(&self, role: &str) -> bool {
        self.entries
            .values()
            .any(|entry| entry.raw["modelRoles"].get(role).is_some())
    }

    /// Whether the selector belongs to an acknowledged native preset.
    pub(super) fn has_assignment(&self, role: &str, selector: &str) -> bool {
        self.entries
            .values()
            .any(|entry| entry.configuration.is_ok() && entry.raw["modelRoles"][role] == selector)
    }

    /// Include native aliases and routing selectors used by saved presets.
    pub(super) fn append_role_options(&self, role: &str, options: &mut Vec<Value>) {
        for selector in self
            .entries
            .values()
            .filter(|entry| entry.configuration.is_ok())
            .filter_map(|entry| entry.raw["modelRoles"][role].as_str())
        {
            if !options.iter().any(|option| option["id"] == selector) {
                options.push(json!({"id":selector,"label":selector}));
            }
        }
    }

    /// Render grouped choices and custom roles without exposing raw settings.
    pub(super) fn append_features(&self, config: &StoredAgentConfig, features: &mut Vec<Value>) {
        let captured = super::captured_roles(config).unwrap_or_default();
        for role in self
            .entries
            .values()
            .filter(|entry| entry.configuration.is_ok())
            .filter_map(|entry| entry.raw["modelRoles"].as_object())
            .flat_map(serde_json::Map::keys)
            .chain(captured.keys())
        {
            if !features
                .iter()
                .any(|feature| feature["id"] == format!("{PREFIX}{role}"))
            {
                let mut options = vec![
                    json!({"id":"native","label":"Use OMP setting"}),
                    json!({"id":"unset","label":"Unassigned"}),
                ];
                self.append_role_options(role, &mut options);
                if let Some(selected) = config
                    .feature_values
                    .as_ref()
                    .and_then(|values| values.get(&format!("{PREFIX}{role}")))
                    .and_then(Value::as_str)
                    && !options.iter().any(|option| option["id"] == selected)
                {
                    options.push(json!({"id":selected,"label":selected}));
                }
                features.push(feature(
                    &Role {
                        id: role.clone(),
                        name: role.clone(),
                    },
                    config,
                    options,
                ));
            }
        }
        let mut options = vec![json!({"id":"__custom__","label":"Custom configuration"})];
        let mut selected = "__custom__";
        for (name, entry) in &self.entries {
            let metadata = match &entry.configuration {
                Ok(preset) => {
                    if selected == "__custom__" && matches_configuration(config, preset) {
                        selected = name;
                    }
                    json!({"source":entry.source,"configuration":{"modelId":preset.model,"thinkingOptionId":preset.thinking_option_id,"featureValues":preset.feature_values}})
                }
                Err(error) => json!({"source":entry.source,"error":error}),
            };
            options.push(
                json!({"id":name,"label":name,"description":entry.source,"metadata":metadata}),
            );
        }
        features.insert(0, json!({"type":"select","id":"modelPreset","label":"OMP model preset","value":selected,"options":options}));
    }

    /// Consume a preset request once; subsequent role edits retain their own values.
    pub(super) fn resolve(
        &self,
        config: &StoredAgentConfig,
    ) -> Result<StoredAgentConfig, AgentSessionError> {
        let Some(name) = config
            .feature_values
            .as_ref()
            .and_then(|values| values.get("modelPreset"))
            .and_then(Value::as_str)
        else {
            return Ok(config.clone());
        };
        if name == "__custom__" {
            let mut config = config.clone();
            if let Some(values) = &mut config.feature_values {
                values.insert("modelPreset".to_owned(), Value::Null);
            }
            return Ok(config);
        }
        let preset = self
            .entries
            .get(name)
            .and_then(|entry| entry.configuration.as_ref().ok())
            .ok_or(AgentSessionError::Rejected)?;
        let mut next = config.clone();
        next.model.clone_from(&preset.model);
        next.thinking_option_id
            .clone_from(&preset.thinking_option_id);
        let values = next.feature_values.get_or_insert_default();
        for (_, value) in values.iter_mut().filter(|(key, _)| is_key(key)) {
            *value = json!("unset");
        }
        values.extend(
            preset
                .feature_values
                .iter()
                .flatten()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        Ok(next)
    }
}

fn matches_configuration(config: &StoredAgentConfig, preset: &StoredAgentConfig) -> bool {
    config.model == preset.model
        && config.thinking_option_id == preset.thinking_option_id
        && preset.feature_values.iter().flatten().all(|(key, value)| {
            key == "modelPreset"
                || config
                    .feature_values
                    .as_ref()
                    .and_then(|values| values.get(key))
                    == Some(value)
        })
}

#[cfg(test)]
mod tests;
