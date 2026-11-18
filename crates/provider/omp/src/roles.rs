//! Native model-role discovery, fallback chains and per-session config overlays.
//!
//! User settings stay owned by OMP. Role assignments persist as `modelRole.<id>` feature values
//! and fallback chains persist as `modelFallback.<id>` arrays; both apply through a private
//! AIT-owned `--config` overlay on the next turn without modifying user native config files.

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
/// Feature-key namespace for per-role ordered fallback model chains.
pub(super) const FALLBACK_PREFIX: &str = "modelFallback.";
/// Stored value meaning "inherit the OMP layered setting" (overlay omits the entry).
pub(super) const INHERIT: &str = "native";
/// Stored value meaning "explicitly unassigned" (overlay writes null for the role).
pub(super) const UNSET: &str = "unset";

const MAX_ROLES: usize = 128;
const MAX_MODELS: usize = 8192;
const MAX_SELECTOR: usize = 2048;
const MAX_ROLE_ID: usize = 128;
const MAX_EFFORTS: usize = 64;
const MAX_FALLBACK_CHAIN: usize = 32;
const MAX_FEATURE_BYTES: usize = 4 * 1024 * 1024;

/// OMP's built-in role identifiers and display names (`packages/coding-agent/src/config/model-roles.ts`).
const BUILTIN_ROLES: [(&str, &str); 16] = [
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

/// OMP's full thinking effort set for reasoning models without per-model effort lists.
const ALL_REASONING_EFFORTS: [&str; 8] = [
    "auto", "off", "minimal", "low", "medium", "high", "xhigh", "max",
];

/// One discovered model from `omp models --kind all --json`.
#[derive(Debug, Clone)]
struct Model {
    selector: String,
    provider: String,
    id: String,
    name: String,
    kind: String,
    input: Vec<String>,
    thinking: Vec<String>,
    web_search: bool,
}

/// One discovered role: native id plus a display label (native tag, OMP built-in name, or raw id).
#[derive(Debug, Clone)]
struct Role {
    id: String,
    name: String,
}

/// Role metadata, inherited selectors/fallbacks and the available model pool of every catalog kind.
#[derive(Debug, Clone, Default)]
pub(super) struct Catalog {
    roles: Vec<Role>,
    inherited: BTreeMap<String, String>,
    inherited_fallbacks: BTreeMap<String, Vec<String>>,
    models: Vec<Model>,
    providers: BTreeSet<String>,
}

impl Catalog {
    /// Query OMP's public machine-readable config and catalog commands concurrently without a
    /// conversation. Uses the exact cwd/environment; malformed, oversized or failed reads error.
    pub(super) async fn read(client: &OmpClient, cwd: &str) -> Result<Self, AgentSessionError> {
        let (settings, models) = tokio::try_join!(
            read(client, cwd, &["config", "list", "--json"]),
            read(client, cwd, &["models", "--kind", "all", "--json"]),
        )?;
        let entry = |key: &str| json!({"key":key,"value":settings[key]["value"]});
        let mut catalog = Self::parse(
            &entry("modelRoles"),
            &entry("modelTags"),
            &entry("cycleOrder"),
            &models,
        )
        .inspect_err(|error| {
            tracing::warn!(
                phase = "role_catalog",
                error = ?error,
                "OMP native metadata validation failed"
            );
        })?;
        catalog.inherited_fallbacks =
            parse_inherited_fallbacks(&settings["retry.fallbackChains"]["value"]);
        // Include custom roles referenced by native presets or fallback chains so an applied
        // snapshot uses the same validation and overlay.
        let preset_roles = settings["modelPresets"]["value"]
            .as_object()
            .into_iter()
            .flat_map(serde_json::Map::values)
            .flat_map(|preset| {
                let role_keys = preset["modelRoles"]
                    .as_object()
                    .into_iter()
                    .flat_map(serde_json::Map::keys);
                let fallback_keys = preset["fallbackChains"]
                    .as_object()
                    .into_iter()
                    .flat_map(serde_json::Map::keys);
                role_keys.chain(fallback_keys)
            });
        let fallback_roles = catalog
            .inherited_fallbacks
            .keys()
            .filter(|key| !key.contains('/'));
        let mut changed = false;
        for role in preset_roles.chain(fallback_roles) {
            if !valid_role(role) {
                return Err(AgentSessionError::Failed);
            }
            if !catalog.roles.iter().any(|known| known.id == *role) {
                if catalog.roles.len() >= MAX_ROLES {
                    return Err(AgentSessionError::Failed);
                }
                catalog.roles.push(Role {
                    id: role.clone(),
                    name: builtin_role_name(role).unwrap_or(role).to_owned(),
                });
                changed = true;
            }
        }
        if changed {
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
        for (builtin, _) in BUILTIN_ROLES {
            if ids.len() >= MAX_ROLES {
                break;
            }
            ids.insert(builtin.to_owned());
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
                    .or_else(|| builtin_role_name(&id))
                    .map_or_else(|| id.clone(), str::to_owned);
                Role { id, name }
            })
            .collect();
        let (parsed_models, providers) = parse_models(models)?;
        Ok(Self {
            roles,
            inherited,
            inherited_fallbacks: BTreeMap::new(),
            models: parsed_models,
            providers,
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

    /// Validate explicit role assignments and fallback chains against this profile's real role
    /// and model pools. Inherited entries remain native; invalid models or kinds reject.
    pub(super) fn validate(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        for (key, value) in config.feature_values.iter().flatten() {
            if let Some(role) = key.strip_prefix(PREFIX) {
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
                continue;
            }
            if let Some(role) = key.strip_prefix(FALLBACK_PREFIX) {
                if value.as_str() == Some(INHERIT) {
                    continue;
                }
                let items = value.as_array().ok_or(AgentSessionError::Rejected)?;
                if items.len() > MAX_FALLBACK_CHAIN
                    || !self.roles.iter().any(|known| known.id == role)
                {
                    return Err(AgentSessionError::Rejected);
                }
                for item in items {
                    let selector = item.as_str().ok_or(AgentSessionError::Rejected)?;
                    if !self.accepts_fallback_selector(role, selector) {
                        return Err(AgentSessionError::Rejected);
                    }
                }
            }
        }
        Ok(())
    }

    fn accepts_fallback_selector(&self, role: &str, selector: &str) -> bool {
        if let Some(alias) = selector.strip_prefix('@') {
            return alias != role && self.roles.iter().any(|known| known.id == alias);
        }
        if let Some(prefix) = selector.strip_suffix("/*") {
            let provider = prefix
                .split_once('/')
                .map_or(prefix, |(provider, _)| provider);
            return !provider.is_empty()
                && self.providers.contains(provider)
                && self
                    .models
                    .iter()
                    .any(|model| model.provider == provider && accepts(role, model));
        }
        self.models
            .iter()
            .any(|model| selected(model, selector) && accepts(role, model))
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
                let inherited_fallbacks = self
                    .inherited_fallbacks
                    .get(&role.id)
                    .cloned()
                    .unwrap_or_default();
                let mut options = vec![json!({
                    "id": INHERIT,
                    "label": inherited,
                    "isDefault": true,
                    "metadata": {
                        "nativeSelector": self.inherited.get(&role.id),
                        "resolvedSelector": resolved,
                        "supportsFallbacks": true,
                        "inheritedFallbacks": inherited_fallbacks,
                    }
                })];
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

fn parse_models(models: &Value) -> Result<(Vec<Model>, BTreeSet<String>), AgentSessionError> {
    let raw_models: Vec<Value> =
        serde_json::from_value(models["models"].clone()).map_err(|_| AgentSessionError::Failed)?;
    if raw_models.len() > MAX_MODELS {
        return Err(AgentSessionError::Failed);
    }
    let mut selectors = BTreeSet::new();
    let mut providers = BTreeSet::new();
    let mut parsed = Vec::with_capacity(raw_models.len());
    for model in &raw_models {
        let selector = text(model, "selector")?;
        if !valid_selector(selector) || !selectors.insert(selector.to_owned()) {
            return Err(AgentSessionError::Failed);
        }
        let (provider, id) = selector.split_once('/').ok_or(AgentSessionError::Failed)?;
        providers.insert(provider.to_owned());
        let name = text(model, "name")?;
        let kind = text(model, "kind")?;
        let input = model["input"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let raw_efforts = parse_efforts(&model["thinking"])?;
        let reasoning = model["reasoning"]
            .as_bool()
            .unwrap_or(!raw_efforts.is_empty());
        let thinking = resolve_model_efforts(reasoning, raw_efforts);
        let web_search = !model["webSearch"].is_null();
        parsed.push(Model {
            selector: selector.to_owned(),
            provider: provider.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            kind: kind.to_owned(),
            input,
            thinking,
            web_search,
        });
    }
    Ok((parsed, providers))
}

fn builtin_role_name(role: &str) -> Option<&'static str> {
    BUILTIN_ROLES
        .iter()
        .find_map(|(id, name)| (*id == role).then_some(*name))
}

fn resolve_model_efforts(reasoning: bool, raw_efforts: Vec<String>) -> Vec<String> {
    if !reasoning {
        return Vec::new();
    }
    if raw_efforts.is_empty() {
        return ALL_REASONING_EFFORTS
            .iter()
            .map(|effort| (*effort).to_owned())
            .collect();
    }
    let mut efforts = Vec::with_capacity(raw_efforts.len() + 2);
    for baseline in ["auto", "off"] {
        if !raw_efforts.iter().any(|effort| effort == baseline) {
            efforts.push(baseline.to_owned());
        }
    }
    efforts.extend(raw_efforts);
    efforts
}

fn parse_inherited_fallbacks(value: &Value) -> BTreeMap<String, Vec<String>> {
    let Some(entries) = value.as_object() else {
        return BTreeMap::new();
    };
    entries
        .iter()
        .take(MAX_ROLES)
        .filter(|(key, _)| valid_role(key))
        .filter_map(|(key, chain)| {
            let items = chain.as_array()?;
            let parsed: Vec<String> = items
                .iter()
                .take(MAX_FALLBACK_CHAIN)
                .filter_map(Value::as_str)
                .filter(|selector| {
                    !selector.trim().is_empty()
                        && selector.len() <= MAX_SELECTOR
                        && !selector.chars().any(char::is_control)
                })
                .map(str::to_owned)
                .collect();
            Some((key.clone(), parsed))
        })
        .collect()
}

/// Whether `key` belongs to native model-role configuration.
pub(super) fn is_key(key: &str) -> bool {
    key.starts_with(PREFIX)
}

/// Whether `key` belongs to native per-role fallback chain configuration.
pub(super) fn is_fallback_key(key: &str) -> bool {
    key.starts_with(FALLBACK_PREFIX)
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

/// Validate one fallback chain feature's bounded shape before native lookup.
pub(super) fn valid_fallback(key: &str, value: &Value) -> bool {
    if !key.strip_prefix(FALLBACK_PREFIX).is_some_and(valid_role) {
        return false;
    }
    if value.as_str() == Some(INHERIT) {
        return true;
    }
    value.as_array().is_some_and(|items| {
        items.len() <= MAX_FALLBACK_CHAIN
            && items.iter().all(|item| {
                item.as_str().is_some_and(|selector| {
                    !selector.trim().is_empty()
                        && selector.len() <= MAX_SELECTOR
                        && !selector.chars().any(char::is_control)
                })
            })
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

/// Return only explicit role assignments; omission preserves OMP's layered settings.
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

/// Return only explicit fallback chain overrides; `"native"` preserves OMP's layered settings.
pub(super) fn fallback_overrides(config: &StoredAgentConfig) -> BTreeMap<String, Vec<String>> {
    config
        .feature_values
        .iter()
        .flatten()
        .filter_map(|(key, value)| {
            let role = key.strip_prefix(FALLBACK_PREFIX)?;
            let items = value.as_array()?;
            let selectors: Vec<String> = items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            Some((role.to_owned(), selectors))
        })
        .collect()
}

/// Whether `config` carries any explicit role or fallback overlay entry.
pub(super) fn has_overrides(config: &StoredAgentConfig) -> bool {
    !overrides(config).is_empty() || !fallback_overrides(config).is_empty()
}

/// Whether two selections produce the same native role and fallback overlay.
pub(super) fn same_overlay(left: &StoredAgentConfig, right: &StoredAgentConfig) -> bool {
    overrides(left) == overrides(right) && fallback_overrides(left) == fallback_overrides(right)
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

/// Materialize a private native `--config` overlay; no file is created when everything inherits.
/// Returns filesystem/serialization failures without editing global or project configuration.
pub(super) async fn overlay(
    config: &StoredAgentConfig,
) -> Result<Option<tempfile::NamedTempFile>, AgentSessionError> {
    let assignments = overrides(config);
    let fallbacks = fallback_overrides(config);
    if assignments.is_empty() && fallbacks.is_empty() {
        return Ok(None);
    }
    let mut overlay = serde_json::Map::new();
    if !assignments.is_empty() {
        let mapped: BTreeMap<_, _> = assignments
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
        overlay.insert("modelRoles".to_owned(), json!(mapped));
    }
    if !fallbacks.is_empty() {
        overlay.insert("retry".to_owned(), json!({ "fallbackChains": fallbacks }));
    }
    let bytes =
        serde_json::to_vec(&Value::Object(overlay)).map_err(|_| AgentSessionError::Rejected)?;
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
    let base = json!({
        "id": model.selector,
        "label": label,
        "metadata": {
            "provider": model.provider,
            "modelId": model.id,
            "efforts": model.thinking,
        }
    });
    std::iter::once(base).chain(model.thinking.iter().map(move |effort| {
        json!({"id":format!("{}:{effort}",model.selector),"label":format!("{label} ({effort})")})
    }))
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
        "value":value,"options":options})
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
    .inspect_err(|error| {
        tracing::warn!(
            phase = args.first().copied(),
            error = ?error,
            "OMP native metadata command failed"
        );
    })?;
    serde_json::from_str(&output).map_err(|_| AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;
