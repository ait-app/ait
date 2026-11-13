//! Native model-role discovery and per-session config overlays; user settings stay owned by OMP.
//!
//! Roles are provider-discovered data, never a fixed enum. The role set is the union of the live
//! profile's `modelRoles`, `modelTags` and `cycleOrder`; labels come from native tags or the raw
//! role id. Role assignments persist as opaque `modelRole.<id>` feature values and apply through a
//! private AIT-owned `--config` overlay on the next turn, never by writing user native config.

pub(super) mod presets;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use serde_json::{Value, json};
use tokio::process::Command;

use super::OmpClient;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::AgentSessionError;
use provider_core::protocol::provider::Details;

/// Feature-key namespace for role assignments.
pub(super) const PREFIX: &str = "modelRole.";
/// Stored value meaning "inherit the OMP layered setting" (overlay omits the role).
pub(super) const INHERIT: &str = "native";
/// Stored value meaning "explicitly unassigned" (overlay writes null for the role).
pub(super) const UNSET: &str = "unset";

const MAX_ROLES: usize = 128;
const MAX_MODELS: usize = 8192;
const MAX_SELECTOR: usize = 2048;
const MAX_ROLE_ID: usize = 128;
const MAX_EFFORTS: usize = 64;
const MAX_FEATURE_BYTES: usize = 768 * 1024;

/// One discovered model from `omp models --kind all --json`.
#[derive(Debug, Clone)]
struct Model {
    selector: String,
    name: String,
    kind: String,
    input: Vec<String>,
    thinking: Vec<String>,
}

/// One discovered role: native id plus a display label (native tag or the id itself).
#[derive(Debug, Clone)]
struct Role {
    id: String,
    name: String,
}

/// Role metadata, inherited selectors and the available model pool of every native catalog kind.
#[derive(Debug, Clone, Default)]
pub(super) struct Catalog {
    roles: Vec<Role>,
    inherited: BTreeMap<String, String>,
    models: Vec<Model>,
}

impl Catalog {
    /// Query OMP's public machine-readable config and catalog commands without a conversation.
    /// Uses the exact cwd/environment; malformed, oversized or failed reads remain errors.
    pub(super) async fn read(client: &OmpClient, cwd: &str) -> Result<Self, AgentSessionError> {
        let settings = read(client, cwd, &["config", "list", "--json"]).await?;
        let models = read(client, cwd, &["models", "--kind", "all", "--json"]).await?;
        let entry = |key: &str| json!({"key":key,"value":settings[key]["value"]});
        let mut catalog = Self::parse(&entry("modelRoles"), &entry("modelTags"), &entry("cycleOrder"), &models)
            .inspect_err(|error| tracing::warn!(phase="role_catalog",error=?error,"OMP native metadata validation failed"))?;
        // A preset may introduce a custom role before it is assigned in the layered settings.
        // Include those native ids so an applied snapshot uses the same validation and overlay.
        if let Some(presets) = settings["modelPresets"]["value"].as_object() {
            for role in presets
                .values()
                .filter_map(|preset| preset["modelRoles"].as_object())
                .flat_map(serde_json::Map::keys)
            {
                if !valid_role(role) {
                    return Err(AgentSessionError::Failed);
                }
                if !catalog.roles.iter().any(|known| known.id == *role) {
                    if catalog.roles.len() >= MAX_ROLES {
                        return Err(AgentSessionError::Failed);
                    }
                    catalog.roles.push(Role {
                        id: role.clone(),
                        name: role.clone(),
                    });
                }
            }
            catalog.roles.sort_by(|left, right| left.id.cmp(&right.id));
        }
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
        if inherited.len() > MAX_ROLES
            || tags.len() > MAX_ROLES
            || cycle.len() > MAX_ROLES
            || inherited
                .iter()
                .any(|(role, selector)| !valid_role(role) || !valid_inherited(selector))
        {
            return Err(AgentSessionError::Failed);
        }
        // Discovered union — no fixed builtin role table.
        let mut ids: BTreeSet<String> = BTreeSet::new();
        for id in cycle.iter().chain(inherited.keys()).chain(tags.keys()) {
            if !valid_role(id) {
                return Err(AgentSessionError::Failed);
            }
            ids.insert(id.clone());
        }
        if ids.len() > MAX_ROLES {
            return Err(AgentSessionError::Failed);
        }
        let roles: Vec<Role> = ids
            .into_iter()
            .map(|id| {
                let name = tags
                    .get(&id)
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .filter(|name| {
                        !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
                    })
                    .map_or_else(|| id.clone(), str::to_owned);
                Role { id, name }
            })
            .collect();
        let raw_models: Vec<Value> = serde_json::from_value(models["models"].clone())
            .map_err(|_| AgentSessionError::Failed)?;
        if raw_models.len() > MAX_MODELS {
            return Err(AgentSessionError::Failed);
        }
        let mut selectors = BTreeSet::new();
        let mut parsed_models = Vec::with_capacity(raw_models.len());
        for model in &raw_models {
            let selector = text(model, "selector")?;
            if !valid_selector(selector) || !selectors.insert(selector.to_owned()) {
                return Err(AgentSessionError::Failed);
            }
            let name = text(model, "name")?;
            let kind = text(model, "kind")?;
            let input = model["input"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let thinking = parse_efforts(&model["thinking"])?;
            parsed_models.push(Model {
                selector: selector.to_owned(),
                name: name.to_owned(),
                kind: kind.to_owned(),
                input,
                thinking,
            });
        }
        Ok(Self {
            roles,
            inherited,
            models: parsed_models,
        })
    }

    fn check_size(&self) -> Result<(), AgentSessionError> {
        if serde_json::to_vec(&self.features(&StoredAgentConfig::default()))
            .map_err(|_| AgentSessionError::Failed)?
            .len()
            > MAX_FEATURE_BYTES
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    /// Validate explicit role assignments against this profile's real role and model pools.
    /// Inherited roles remain native; invalid models or incompatible catalog kinds reject.
    pub(super) fn validate(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        for (key, value) in config.feature_values.iter().flatten() {
            let Some(role) = key.strip_prefix(PREFIX) else {
                continue;
            };
            let value = value.as_str().ok_or(AgentSessionError::Rejected)?;
            if value == INHERIT {
                continue;
            }
            let known_role = self.roles.iter().any(|known| known.id == role);
            let known_model = self
                .models
                .iter()
                .any(|model| selected(model, value) && accepts(role, model));
            if !known_role || (value != UNSET && !known_model) {
                return Err(AgentSessionError::Rejected);
            }
        }
        Ok(())
    }

    /// Render standard select controls with actual native choices and explicit inheritance.
    pub(super) fn features(&self, config: &StoredAgentConfig) -> Vec<Value> {
        let mut features: Vec<Value> = self
            .roles
            .iter()
            .map(|role| {
                let inherited = self.inherited.get(&role.id).map_or_else(
                    || "Use OMP setting".to_owned(),
                    |selector| format!("Use OMP setting ({selector})"),
                );
                let resolved = self.resolve_alias(&role.id);
                let mut options = vec![json!({"id":INHERIT,"label":inherited,"isDefault":true,
                    "metadata":{"nativeSelector":self.inherited.get(&role.id),
                        "resolvedSelector":resolved}})];
                options.push(json!({"id":UNSET,"label":"Unassigned"}));
                options.extend(
                    self.models
                        .iter()
                        .filter(|model| accepts(&role.id, model))
                        .flat_map(model_options),
                );
                feature(role, config, options)
            })
            .collect();
        features.extend(stored_features(config).into_iter().filter(|feature| {
            let Some(id) = feature["id"]
                .as_str()
                .and_then(|id| id.strip_prefix(PREFIX))
            else {
                return false;
            };
            !self.roles.iter().any(|role| role.id == id)
        }));
        features
    }

    /// Append role controls without altering the current conversation's model catalog.
    pub(super) fn append(&self, details: &mut Details, config: &StoredAgentConfig) {
        details.features.extend(self.features(config));
    }

    /// Bounded `@alias` resolution for inheritance display; cycles leave the selector unresolved.
    fn resolve_alias(&self, role: &str) -> Option<String> {
        let mut current = self.inherited.get(role)?.clone();
        for _ in 0..=self.roles.len() {
            match current.strip_prefix('@') {
                Some(alias) => current.clone_from(self.inherited.get(alias)?),
                None => return Some(current),
            }
        }
        None
    }
}

/// Whether `key` belongs to native model-role configuration.
pub(super) fn is_key(key: &str) -> bool {
    key.starts_with(PREFIX)
}

/// Validate one role feature's bounded shape before native lookup.
pub(super) fn valid(key: &str, value: &Value) -> bool {
    key.strip_prefix(PREFIX).is_some_and(valid_role)
        && value.as_str().is_some_and(|value| {
            matches!(value, INHERIT | UNSET)
                || (!value.is_empty()
                    && value.len() <= MAX_SELECTOR
                    && !value.chars().any(char::is_control))
        })
}

fn valid_role(role: &str) -> bool {
    !role.is_empty() && role.len() <= MAX_ROLE_ID && !role.chars().any(char::is_control)
}

fn valid_selector(selector: &str) -> bool {
    !selector.is_empty()
        && selector.len() <= MAX_SELECTOR
        && !selector.chars().any(char::is_control)
        && selector
            .split_once('/')
            .is_some_and(|(provider, id)| !provider.is_empty() && !id.is_empty())
}

/// An inherited selector may be a `provider/id` route or an `@alias` to another role.
fn valid_inherited(selector: &str) -> bool {
    if let Some(alias) = selector.strip_prefix('@') {
        return valid_role(alias);
    }
    valid_selector(selector)
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

/// Whether two selections produce the same native role overlay.
pub(super) fn same_overlay(left: &StoredAgentConfig, right: &StoredAgentConfig) -> bool {
    overrides(left) == overrides(right)
}

/// Render persisted overrides before the native owner or catalog is available.
/// Stored keys only — no fixed builtin role set is invented offline.
pub(super) fn stored_features(config: &StoredAgentConfig) -> Vec<Value> {
    let mut roles: BTreeMap<String, String> = BTreeMap::new();
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
                options.push(json!({"id":value,"label":value,"metadata":{"unresolved":true}}));
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
    if assignments.is_empty() {
        return Ok(None);
    }
    let assignments: BTreeMap<_, _> = assignments
        .into_iter()
        .map(|(role, selector)| {
            (
                role,
                if selector == UNSET {
                    Value::Null
                } else {
                    json!(selector)
                },
            )
        })
        .collect();
    let overlay = json!({"modelRoles":assignments});
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

fn selected(model: &Model, value: &str) -> bool {
    value == model.selector
        || value
            .strip_prefix(&model.selector)
            .and_then(|suffix| suffix.strip_prefix(':'))
            .is_some_and(|effort| model.thinking.iter().any(|known| known == effort))
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
        model.thinking.iter().map(move |effort| {
            json!({"id":format!("{}:{effort}",model.selector),"label":format!("{label} ({effort})")})
        }),
    )
}

fn accepts(role: &str, model: &Model) -> bool {
    match role {
        "image" => model.kind == "image",
        "web" => model.kind == "search",
        "speech" => model.kind == "tts",
        "dictation" => model.kind == "stt",
        "tiny" | "memory" => matches!(model.kind.as_str(), "tiny" | "chat"),
        "judge" => matches!(model.kind.as_str(), "judge" | "tiny" | "chat"),
        _ => model.kind == "chat",
    }
}

fn feature(role: &Role, config: &StoredAgentConfig, mut options: Vec<Value>) -> Value {
    let id = format!("{PREFIX}{}", role.id);
    let value = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get(&id))
        .and_then(Value::as_str)
        .unwrap_or(INHERIT);
    if !options.iter().any(|option| option["id"] == value) {
        options.push(json!({"id":value,"label":value,"metadata":{"unresolved":true}}));
    }
    json!({"type":"select","id":id,"label":format!("{} model ({})",role.name,role.id),
        "description":"Applies to the next turn","value":value,"options":options})
}

fn parse_efforts(value: &Value) -> Result<Vec<String>, AgentSessionError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let efforts = value.as_array().ok_or(AgentSessionError::Failed)?;
    if efforts.len() > MAX_EFFORTS {
        return Err(AgentSessionError::Failed);
    }
    efforts
        .iter()
        .map(|effort| {
            effort
                .as_str()
                .filter(|effort| {
                    !effort.is_empty()
                        && effort.len() <= 64
                        && effort
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                })
                .map(str::to_owned)
                .ok_or(AgentSessionError::Failed)
        })
        .collect()
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

async fn read(client: &OmpClient, cwd: &str, args: &[&str]) -> Result<Value, AgentSessionError> {
    let mut command = Command::new(&client.program);
    command
        .args(args)
        .current_dir(cwd)
        .envs(client.environment.entries());
    let output = tokio::time::timeout(
        client.timeouts.request,
        provider_native::metadata_process::output(&mut command),
    )
    .await
    .map_err(|_| AgentSessionError::Failed)
    .flatten()
    .inspect_err(|error| tracing::warn!(phase=args.first().copied(),error=?error,"OMP native metadata command failed"))?;
    serde_json::from_str(&output).map_err(|_| AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
