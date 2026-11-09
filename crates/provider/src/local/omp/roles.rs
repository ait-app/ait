//! Native model-role discovery and per-session config overlays; user settings stay owned by OMP.

pub(super) mod presets;

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::Command;

use super::OmpClient;
use crate::{ports::agent_session::AgentSessionError, protocol::provider::Details};
use domain::agent_runtime::StoredAgentConfig;

const PREFIX: &str = "modelRole.";
const INHERIT: &str = "native";
const BUILTINS: &[(&str, &str)] = &[
    ("default", "Default"),
    ("smol", "Fast"),
    ("slow", "Thinking"),
    ("vision", "Vision"),
    ("plan", "Architect"),
    ("commit", "Commit"),
    ("tiny", "Tiny"),
    ("memory", "Memory"),
    ("task", "Subtask"),
    ("advisor", "Advisor"),
    ("widget", "Job widgets"),
    ("image", "Image generation"),
    ("web", "Web search"),
    ("speech", "Speech"),
    ("dictation", "Dictation"),
    ("judge", "Judge"),
];

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Model {
    selector: String,
    name: String,
    kind: String,
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    thinking: Option<Vec<String>>,
    #[serde(skip)]
    web_search: bool,
}

#[derive(Debug, Clone)]
struct Role {
    id: String,
    name: String,
}

/// Role metadata, original inherited selectors and available models of every native catalog kind.
#[derive(Debug, Clone)]
pub(super) struct Catalog {
    roles: Vec<Role>,
    inherited: BTreeMap<String, String>,
    models: Vec<Model>,
    presets: presets::Presets,
}

impl Catalog {
    /// Resolve a worker role's explicit thinking suffix, following bounded native role aliases.
    /// Missing, unset or cyclic selections leave thinking to the worker model's native default.
    pub(super) fn worker_thinking(&self, config: &StoredAgentConfig, role: &str) -> Option<String> {
        let assignments = overrides(config);
        let selector = |role: &str| assignments.get(role).or_else(|| self.inherited.get(role));
        let mut current = selector(role)?;
        for _ in 0..=self.roles.len() {
            if let Some((_, effort)) = current.rsplit_once(':')
                && presets::valid_thinking(effort)
            {
                return Some(effort.to_owned());
            }
            current = selector(current.strip_prefix('@')?)?;
        }
        None
    }

    /// Query OMP's public machine-readable config and catalog commands without a conversation.
    /// Uses the exact cwd/environment; malformed, oversized or failed reads remain errors.
    pub(super) async fn read(client: &OmpClient, cwd: &str) -> Result<Self, AgentSessionError> {
        // OMP initializes shared profile caches on first use, even for metadata commands.
        // Finish that bootstrap before starting another reader of the same profile.
        let settings = read(client, cwd, &["config", "list", "--json"]).await?;
        let models = read(client, cwd, &["models", "--kind", "all", "--json"]).await?;
        // Native list redacts credentials. Retain only this explicit configuration allowlist.
        let entry = |key: &str| json!({"key":key,"value":settings[key]["value"]});
        let mut catalog = Self::parse(
            &entry("modelRoles"),
            &entry("modelTags"),
            &entry("cycleOrder"),
            &models,
        ).inspect_err(|error|tracing::warn!(phase="role_catalog",error=?error,"OMP native metadata validation failed"))?;
        catalog.presets = presets::read(client, cwd, &settings, &catalog).await?;
        catalog.check_size()?;
        Ok(catalog)
    }

    fn parse(
        roles: &Value,
        tags: &Value,
        cycle: &Value,
        models: &Value,
    ) -> Result<Self, AgentSessionError> {
        if roles["key"] != "modelRoles"
            || tags["key"] != "modelTags"
            || cycle["key"] != "cycleOrder"
        {
            return Err(AgentSessionError::Failed);
        }
        let inherited: BTreeMap<String, String> = serde_json::from_value(roles["value"].clone())
            .map_err(|_| AgentSessionError::Failed)?;
        let tags = tags["value"].as_object().ok_or(AgentSessionError::Failed)?;
        let cycle: Vec<String> = serde_json::from_value(cycle["value"].clone())
            .map_err(|_| AgentSessionError::Failed)?;
        if inherited.len() > 128
            || tags.len() > 128
            || cycle.len() > 128
            || inherited.iter().any(|(role, selector)| {
                !valid_role(role)
                    || selector.is_empty()
                    || selector.len() > 2048
                    || selector.chars().any(char::is_control)
            })
        {
            return Err(AgentSessionError::Failed);
        }
        let mut roles: Vec<Role> = BUILTINS
            .iter()
            .map(|(id, name)| Role {
                id: (*id).into(),
                name: (*name).into(),
            })
            .collect();
        let mut known: BTreeSet<String> = roles.iter().map(|role| role.id.clone()).collect();
        for id in cycle.iter().chain(inherited.keys()).chain(tags.keys()) {
            if !valid_role(id) {
                return Err(AgentSessionError::Failed);
            }
            if known.insert(id.clone()) {
                roles.push(Role {
                    id: id.clone(),
                    name: id.clone(),
                });
            }
        }
        for role in &mut roles {
            if let Some(name) = tags.get(&role.id).and_then(|value| value.get("name")) {
                if name == "" {
                    continue;
                }
                let name = name
                    .as_str()
                    .filter(|name| {
                        !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
                    })
                    .ok_or(AgentSessionError::Failed)?;
                role.name = name.to_owned();
            }
        }
        let models: Vec<Model> = serde_json::from_value(models["models"].clone())
            .map_err(|_| AgentSessionError::Failed)?;
        let mut selectors = BTreeSet::new();
        if models.len() > 8192
            || models.iter().any(|model| {
                !valid_selector(&model.selector)
                    || model.name.is_empty()
                    || model.name.len() > 1024
                    || model.name.chars().any(char::is_control)
                    || model.thinking.as_ref().is_some_and(|efforts| {
                        efforts.len() > 64
                            || efforts.iter().any(|effort| {
                                effort.is_empty()
                                    || effort.len() > 64
                                    || !effort
                                        .bytes()
                                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                            })
                    })
                    || !selectors.insert(model.selector.clone())
            })
        {
            return Err(AgentSessionError::Failed);
        }
        let catalog = Self {
            roles,
            inherited,
            models,
            presets: presets::Presets::default(),
        };
        catalog.check_size()?;
        Ok(catalog)
    }

    fn check_size(&self) -> Result<(), AgentSessionError> {
        // The existing select-feature wire format repeats options per role; bound the whole surface.
        if serde_json::to_vec(&self.features(&StoredAgentConfig::default()))
            .map_err(|_| AgentSessionError::Failed)?
            .len()
            > 768 * 1024
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    /// Validate explicit role assignments against this profile's real role and model pools.
    /// Inherited roles remain native; invalid models or incompatible catalog kinds reject.
    pub(super) fn validate(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        let captured = captured_roles(config)?;
        for (key, value) in config.feature_values.iter().flatten() {
            let Some(role) = key.strip_prefix(PREFIX) else {
                continue;
            };
            let value = value.as_str().ok_or(AgentSessionError::Rejected)?;
            if !self.roles.iter().any(|known| known.id == role)
                && !self.presets.has_role(role)
                && !captured.contains_key(role)
                && !matches!(value, INHERIT | "unset")
            {
                return Err(AgentSessionError::Rejected);
            }
            if !matches!(value, INHERIT | "unset")
                && !self.presets.has_assignment(role, value)
                && captured.get(role).map(String::as_str) != Some(value)
                && !self
                    .models
                    .iter()
                    .any(|model| selected(model, value) && accepts(role, model))
            {
                return Err(AgentSessionError::Rejected);
            }
        }
        Ok(())
    }

    /// Render standard select controls with actual native choices and explicit inheritance.
    pub(super) fn features(&self, config: &StoredAgentConfig) -> Vec<Value> {
        let mut features: Vec<_> = self
            .roles
            .iter()
            .map(|role| {
                let inherited = self.inherited.get(&role.id).map_or_else(
                    || "Use OMP setting".to_owned(),
                    |selector| format!("Use OMP setting ({selector})"),
                );
                let mut options = vec![json!({"id":INHERIT,"label":inherited,"isDefault":true})];
                if self.presets.has_entries()
                    || config
                        .feature_values
                        .as_ref()
                        .and_then(|values| values.get(&format!("{PREFIX}{}", role.id)))
                        == Some(&json!("unset"))
                {
                    options.push(json!({"id":"unset","label":"Unassigned"}));
                }
                options.extend(
                    self.models
                        .iter()
                        .filter(|model| accepts(&role.id, model))
                        .flat_map(model_options),
                );
                self.presets.append_role_options(&role.id, &mut options);
                feature(role, config, options)
            })
            .collect();
        self.presets.append_features(config, &mut features);
        features
    }

    /// Expand an explicit preset request into one stable, private next-turn configuration.
    /// Missing, malformed or unavailable presets reject before any settings are persisted.
    pub(super) fn resolve_preset(
        &self,
        config: &StoredAgentConfig,
    ) -> Result<StoredAgentConfig, AgentSessionError> {
        self.presets.resolve(config)
    }

    /// Native presets as unified editor records; only acknowledged entries are listed.
    pub(super) fn preset_records(&self) -> Vec<crate::ports::preset::PresetEntry> {
        self.presets.records()
    }

    /// Append role controls without altering the current conversation's model catalog.
    pub(super) fn append(&self, details: &mut Details, config: &StoredAgentConfig) {
        details.features.extend(self.features(config));
    }

    /// Enrich chat-only search capability from an existing disposable native reader.
    /// Returns malformed/bounded metadata errors without retaining raw provider fields.
    pub(super) async fn read_chat(
        &mut self,
        probe: &mut super::transport::Transport,
    ) -> Result<(), AgentSessionError> {
        let result = probe.request("get_available_models", json!({})).await?;
        let models = result["models"]
            .as_array()
            .filter(|models| models.len() <= 8192)
            .ok_or(AgentSessionError::Failed)?;
        let web_models: BTreeSet<String> = models
            .iter()
            .filter(|model| !model["webSearch"].is_null())
            .map(|model| {
                Ok(format!(
                    "{}/{}",
                    super::config::text(model, "provider")?,
                    super::config::text(model, "id")?
                ))
            })
            .collect::<Result<_, AgentSessionError>>()?;
        for model in &mut self.models {
            model.web_search = web_models.contains(&model.selector);
        }
        self.check_size()
    }
}

fn selected(model: &Model, value: &str) -> bool {
    value == model.selector
        || value
            .strip_prefix(&model.selector)
            .and_then(|suffix| suffix.strip_prefix(':'))
            .is_some_and(|effort| {
                model
                    .thinking
                    .as_ref()
                    .is_some_and(|efforts| efforts.iter().any(|known| known == effort))
            })
}

fn model_options(model: &Model) -> impl Iterator<Item = Value> + '_ {
    let label = format!(
        "{}{} · {}",
        model.name,
        if model.input.iter().any(|input| input == "image") {
            " (images)"
        } else {
            ""
        },
        model.selector
    );
    std::iter::once(json!({"id":model.selector,"label":label})).chain(
        model.thinking.iter().flatten().map(move |effort|json!({"id":format!("{}:{effort}",model.selector),"label":format!("{label} ({effort})")})))
}

fn accepts(role: &str, model: &Model) -> bool {
    match role {
        "image" => model.kind == "image",
        "web" => model.kind == "search" || (model.kind == "chat" && model.web_search),
        "speech" => model.kind == "tts",
        "dictation" => model.kind == "stt",
        "tiny" | "memory" => matches!(model.kind.as_str(), "tiny" | "chat"),
        "judge" => matches!(model.kind.as_str(), "judge" | "tiny" | "chat"),
        _ => model.kind == "chat",
    }
}

fn feature(role: &Role, config: &StoredAgentConfig, options: Vec<Value>) -> Value {
    let id = format!("{PREFIX}{}", role.id);
    let value = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get(&id))
        .and_then(Value::as_str)
        .unwrap_or(INHERIT);
    let mut feature = json!({"type":"select","id":id,"label":format!("{} model ({})",role.name,role.id),"value":value});
    feature["options"] = Value::Array(options);
    feature
}

/// Whether `key` belongs to native model-role configuration.
pub(super) fn is_key(key: &str) -> bool {
    key.starts_with(PREFIX)
}

/// Validate native preset feature values before an owning-directory lookup.
pub(super) fn valid_preset(key: &str, value: &Value) -> bool {
    match key {
        "modelPreset" => {
            value.is_null()
                || value
                    .as_str()
                    .is_some_and(|name| name == "__custom__" || presets::valid_name(name))
        }
        "modelPresetThinking" => {
            value.is_null() || value.as_str().is_some_and(presets::valid_thinking)
        }
        "modelPresetRoles" => value
            .as_str()
            .is_some_and(|value| parse_captured_roles(value).is_ok()),
        _ => false,
    }
}

fn parse_captured_roles(value: &str) -> Result<BTreeMap<String, String>, AgentSessionError> {
    if value.len() > 65536 {
        return Err(AgentSessionError::Rejected);
    }
    let roles: BTreeMap<String, String> =
        serde_json::from_str(value).map_err(|_| AgentSessionError::Rejected)?;
    if roles.len() > 128
        || roles
            .iter()
            .any(|(role, selector)| !valid(&format!("{PREFIX}{role}"), &json!(selector)))
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(roles)
}

fn captured_roles(
    config: &StoredAgentConfig,
) -> Result<BTreeMap<String, String>, AgentSessionError> {
    config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("modelPresetRoles"))
        .and_then(Value::as_str)
        .map(parse_captured_roles)
        .transpose()
        .map(Option::unwrap_or_default)
}

/// Run an explicit preset save/delete through an isolated official native owner.
/// The future changes only the named global preset; role selection remains private to Ait.
pub(super) fn manage_preset(
    client: OmpClient,
    cwd: String,
    config: StoredAgentConfig,
    action: &str,
    arguments: &Value,
) -> Result<crate::ports::agent_session::AgentSessionFuture<'static, Value>, AgentSessionError> {
    presets::management::begin(client, cwd, config, action, arguments)
}

/// Validate one role feature's bounded shape before native lookup.
pub(super) fn valid(key: &str, value: &Value) -> bool {
    key.strip_prefix(PREFIX).is_some_and(valid_role)
        && value.as_str().is_some_and(|value| {
            matches!(value, INHERIT | "unset")
                || (!value.is_empty()
                    && value.len() <= 2048
                    && !value.chars().any(char::is_control))
        })
}

fn valid_role(role: &str) -> bool {
    !role.is_empty() && role.len() <= 128 && !role.chars().any(char::is_control)
}

fn valid_selector(selector: &str) -> bool {
    !selector.is_empty()
        && selector.len() <= 2048
        && !selector.chars().any(char::is_control)
        && selector
            .split_once('/')
            .is_some_and(|(provider, id)| !provider.is_empty() && !id.is_empty())
}

/// Return only explicit assignments; omission preserves OMP's layered settings and effort suffixes.
pub(super) fn overrides(config: &StoredAgentConfig) -> BTreeMap<String, String> {
    config
        .feature_values
        .iter()
        .flatten()
        .filter_map(|(key, value)| {
            let role = key.strip_prefix(PREFIX)?;
            let value = value.as_str().filter(|value| *value != INHERIT)?;
            Some((role.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Whether two selections produce the same native role and default-thinking overlay.
pub(super) fn same_overlay(left: &StoredAgentConfig, right: &StoredAgentConfig) -> bool {
    let thinking = |config: &StoredAgentConfig| {
        config
            .feature_values
            .as_ref()
            .and_then(|values| values.get("modelPresetThinking"))
            .filter(|value| !value.is_null())
            .cloned()
    };
    overrides(left) == overrides(right) && thinking(left) == thinking(right)
}

/// Render persisted overrides before the native owner or catalog is available.
pub(super) fn stored_features(config: &StoredAgentConfig) -> Vec<Value> {
    let mut roles: BTreeMap<String, String> = BUILTINS
        .iter()
        .map(|(id, label)| ((*id).into(), (*label).into()))
        .collect();
    for (key, _) in config.feature_values.iter().flatten() {
        if let Some(role) = key.strip_prefix(PREFIX) {
            roles
                .entry(role.to_owned())
                .or_insert_with(|| role.to_owned());
        }
    }
    roles
        .into_iter()
        .map(|(id, name)| {
            let key = format!("{PREFIX}{id}");
            let mut options =
                vec![json!({"id":INHERIT,"label":"Use OMP setting","isDefault":true})];
            if let Some(value) = config
                .feature_values
                .as_ref()
                .and_then(|values| values.get(&key))
                .and_then(Value::as_str)
                .filter(|value| *value != INHERIT)
            {
                options.push(json!({"id":value,"label":value}));
            }
            feature(&Role { id, name }, config, options)
        })
        .collect()
}

/// Materialize a private native `--config` overlay; no file is created for inherited roles.
/// Returns filesystem/serialization failures without editing global or project configuration.
pub(super) async fn overlay(
    config: &StoredAgentConfig,
) -> Result<Option<tempfile::NamedTempFile>, AgentSessionError> {
    let assignments = overrides(config);
    let thinking = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get("modelPresetThinking"));
    if assignments.is_empty() && thinking.is_none_or(Value::is_null) {
        return Ok(None);
    }
    let assignments: BTreeMap<_, _> = assignments
        .into_iter()
        .map(|(role, selector)| {
            (
                role,
                if selector == "unset" {
                    Value::Null
                } else {
                    json!(selector)
                },
            )
        })
        .collect();
    let mut overlay = json!({"modelRoles":assignments});
    if let Some(thinking) = thinking.filter(|value| !value.is_null()) {
        overlay["defaultThinkingLevel"] = thinking.clone();
    }
    let bytes = serde_json::to_vec(&overlay).map_err(|_| AgentSessionError::Rejected)?;
    tokio::task::spawn_blocking(move || {
        let mut file = tempfile::Builder::new()
            .prefix("ait-omp-roles-")
            .suffix(".json")
            .tempfile()
            .map_err(|_| AgentSessionError::Failed)?;
        file.write_all(&bytes)
            .map_err(|_| AgentSessionError::Failed)?;
        file.flush().map_err(|_| AgentSessionError::Failed)?;
        Ok(Some(file))
    })
    .await
    .map_err(|_| AgentSessionError::Failed)?
}

async fn read(client: &OmpClient, cwd: &str, args: &[&str]) -> Result<Value, AgentSessionError> {
    let mut command = Command::new(&client.program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(client.environment.entries());
    let output = tokio::time::timeout(
        super::transport::STARTUP_TIMEOUT.max(client.deadline),
        crate::local::metadata_process::output(&mut command),
    )
    .await
    .map_err(|_| AgentSessionError::Failed)
    .flatten()
    .inspect_err(|error|tracing::warn!(phase=args.first().copied(),error=?error,"OMP native metadata command failed"))?;
    serde_json::from_str(&output).map_err(|_| AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
