//! OMP's native preset store through its public, non-conversational configuration CLI.
//!
//! Reads preserve whole-layer project precedence. Explicit user edits update only the global
//! `modelPresets` setting; applying a preset still uses the existing private role overlay.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{
    Catalog, FALLBACK_PREFIX, INHERIT, MAX_FALLBACK_CHAIN, PREFIX, UNSET, selected, valid_role,
};
use crate::{OmpClient, config};
use provider_core::ports::preset::{
    Preset, PresetBackend, PresetDescriptor, PresetEntry, PresetError, PresetSource,
};

const MAX_PRESETS: usize = 128;
const MAX_STORE_BYTES: usize = 768 * 1024;
const READ_ERROR: &str =
    "OMP native presets could not be read; check the installed OMP configuration CLI";

/// Describe the official native store without creating a conversation or changing settings.
pub(crate) async fn describe(client: &OmpClient, cwd: &str) -> PresetDescriptor {
    let available = async {
        let read_key = async {
            let setting = super::read(client, cwd, &["config", "get", "modelPresets", "--json"])
                .await
                .map_err(|_| PresetError::Unavailable(READ_ERROR.to_owned()))?;
            if setting["key"] != "modelPresets" {
                return Err(PresetError::Unavailable(READ_ERROR.to_owned()));
            }
            Ok(())
        };
        tokio::try_join!(read_key, layers(client, cwd))
    };
    match available.await {
        Ok(_) => PresetDescriptor::managed(PresetBackend::Native),
        Err(error) => PresetDescriptor::unavailable(PresetBackend::Native, error.message()),
    }
}

/// List native global and project definitions with each project's whole-record override.
///
/// Reuses `OmpClient`'s cached role catalog concurrently with layer reads so opening the picker
/// or settings editor does not spawn redundant `omp models --kind all` subprocesses.
pub(crate) async fn list(client: &OmpClient, cwd: &str) -> Result<Vec<PresetEntry>, PresetError> {
    let (layers_result, catalog_result) =
        tokio::join!(layers(client, cwd), client.role_catalog(cwd));
    let (global, project) = layers_result?;
    let catalog = catalog_result.map_err(|_| PresetError::Unavailable(READ_ERROR.to_owned()))?;
    let mut owned: BTreeMap<_, _> = global
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .map(|(name, value)| (name, (value, PresetSource::Global)))
        .collect();
    for (name, value) in project {
        if value.is_null() {
            owned.remove(&name);
        } else {
            owned.insert(name, (value, PresetSource::Project));
        }
    }
    owned
        .into_iter()
        .map(|(name, (raw, source))| {
            let preset = record(&catalog, &name, &raw)?;
            Ok(PresetEntry {
                preset,
                source,
                editable: source == PresetSource::Global,
            })
        })
        .collect()
}

/// Save one native global preset and verify OMP acknowledged exactly that definition.
///
/// Returns validation, project ownership or native command failures without changing any
/// conversation's model, permissions, tools or role overlay.
pub(crate) async fn save(
    client: &OmpClient,
    cwd: &str,
    preset: &Preset,
) -> Result<(), PresetError> {
    check_name(&preset.name)?;
    preset.validate().map_err(PresetError::Rejected)?;
    let _guard = client.preset_writes.lock().await;
    let (layers_result, catalog_result) =
        tokio::join!(layers(client, cwd), client.role_catalog(cwd));
    let (mut global, project) = layers_result?;
    check_global(&project, &preset.name)?;
    let catalog = catalog_result.map_err(|_| PresetError::Unavailable(READ_ERROR.to_owned()))?;
    let raw = match definition(&catalog, preset, global.get(&preset.name)) {
        Ok(raw) => raw,
        Err(PresetError::Rejected(_)) => {
            // An external native config edit may have added a custom role or model since the
            // directory catalog was cached; refresh once before rejecting.
            client.invalidate_role_catalog(cwd);
            let fresh = client
                .role_catalog(cwd)
                .await
                .map_err(|_| PresetError::Unavailable(READ_ERROR.to_owned()))?;
            definition(&fresh, preset, global.get(&preset.name))?
        }
        Err(error) => return Err(error),
    };
    global.insert(preset.name.clone(), raw);
    write(client, cwd, global).await?;
    client.invalidate_all_role_catalogs();
    Ok(())
}

/// Delete one global definition, refusing a same-name project definition.
///
/// Returns an unknown name or command failure without changing other presets.
pub(crate) async fn delete(client: &OmpClient, cwd: &str, name: &str) -> Result<(), PresetError> {
    check_name(name)?;
    let _guard = client.preset_writes.lock().await;
    let (mut global, project) = layers(client, cwd).await?;
    check_global(&project, name)?;
    if global.remove(name).is_none() {
        return Err(PresetError::Rejected(format!(
            "OMP preset {name:?} is not defined globally"
        )));
    }
    write(client, cwd, global).await?;
    client.invalidate_all_role_catalogs();
    Ok(())
}

fn check_name(name: &str) -> Result<(), PresetError> {
    if !name.is_empty()
        && name.len() <= 128
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Ok(());
    }
    Err(PresetError::Rejected(
        "OMP preset names must start with a letter and contain only letters, digits, '-' or '_'"
            .to_owned(),
    ))
}

fn check_global(project: &Map<String, Value>, name: &str) -> Result<(), PresetError> {
    if project.contains_key(name) {
        return Err(PresetError::Rejected(
            "Project presets are read-only here; the same-name global preset was not changed"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn layers(
    client: &OmpClient,
    cwd: &str,
) -> Result<(Map<String, Value>, Map<String, Value>), PresetError> {
    let (global, project) = tokio::join!(
        read_layer(client, cwd, "global"),
        read_layer(client, cwd, "project"),
    );
    Ok((global?, project?))
}

async fn read_layer(
    client: &OmpClient,
    cwd: &str,
    scope: &str,
) -> Result<Map<String, Value>, PresetError> {
    let output = super::read(client, cwd, &["config", "list", "--scope", scope, "--json"])
        .await
        .map_err(|_| PresetError::Unavailable(READ_ERROR.to_owned()))?;
    if !output.is_object() {
        return Err(PresetError::Unavailable(READ_ERROR.to_owned()));
    }
    let value = &output["modelPresets"]["value"];
    if value.is_null() {
        return Ok(Map::new());
    }
    let entries = value
        .as_object()
        .ok_or_else(|| PresetError::Unavailable(READ_ERROR.to_owned()))?;
    if entries.len() > MAX_PRESETS {
        return Err(PresetError::Unavailable(
            "OMP has too many native presets".to_owned(),
        ));
    }
    Ok(entries.clone())
}

async fn write(
    client: &OmpClient,
    cwd: &str,
    entries: Map<String, Value>,
) -> Result<(), PresetError> {
    if entries.len() > MAX_PRESETS {
        return Err(PresetError::Rejected(
            "OMP has too many native presets".to_owned(),
        ));
    }
    let serialized = serde_json::to_string(&entries)
        .map_err(|_| PresetError::Rejected("OMP presets cannot be serialized".to_owned()))?;
    if serialized.len() > MAX_STORE_BYTES {
        return Err(PresetError::Rejected(
            "OMP presets exceed the configuration budget".to_owned(),
        ));
    }
    // Only the preset map is written. In particular this never writes the modelRoles setting
    // or starts a session; OMP itself owns serialization and persistence of its configuration.
    super::read(
        client,
        cwd,
        &["config", "set", "modelPresets", &serialized, "--json"],
    )
    .await
    .map_err(|_| {
        PresetError::Unavailable("OMP did not acknowledge the preset change".to_owned())
    })?;
    if read_layer(client, cwd, "global").await? != entries {
        return Err(PresetError::Unavailable(
            "OMP did not persist the requested preset change".to_owned(),
        ));
    }
    Ok(())
}

fn record(catalog: &Catalog, name: &str, raw: &Value) -> Result<Preset, PresetError> {
    let reject = || {
        PresetError::Unavailable(format!(
            "OMP preset {name:?} has an invalid or unresolved model-role configuration"
        ))
    };
    let roles = raw["modelRoles"].as_object().ok_or_else(reject)?;
    if roles.len() > super::MAX_ROLES || roles.keys().any(|role| !valid_role(role)) {
        return Err(reject());
    }
    let default = resolve(catalog, roles, "default")
        .or_else(|| {
            roles
                .get("default")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(reject)?;
    let (model, effort) = model_route(catalog, &default);
    let thinking = raw
        .get("defaultThinkingLevel")
        .filter(|value| !value.is_null())
        .map(|value| value.as_str().ok_or_else(reject))
        .transpose()?
        .or(effort);
    let mut feature_values: BTreeMap<String, Value> = roles
        .iter()
        .filter(|(role, _)| role.as_str() != "default")
        .map(|(role, value)| {
            let assignment = if value.is_null() {
                json!(UNSET)
            } else if value.is_string() {
                json!(
                    resolve(catalog, roles, role)
                        .or_else(|| value.as_str().map(str::to_owned))
                        .ok_or_else(reject)?
                )
            } else {
                value.clone()
            };
            Ok((format!("{PREFIX}{role}"), assignment))
        })
        .collect::<Result<_, PresetError>>()?;
    if let Some(fallbacks) = raw.get("fallbackChains").filter(|value| !value.is_null()) {
        let chains = fallbacks.as_object().ok_or_else(reject)?;
        if chains.len() > super::MAX_ROLES || chains.keys().any(|role| !valid_role(role)) {
            return Err(reject());
        }
        for (role, chain) in chains {
            let items = chain.as_array().ok_or_else(reject)?;
            if items.len() > MAX_FALLBACK_CHAIN {
                return Err(reject());
            }
            let selectors: Vec<Value> = items
                .iter()
                .map(|item| {
                    let selector = item
                        .as_str()
                        .filter(|text| {
                            !text.trim().is_empty()
                                && text.len() <= super::MAX_SELECTOR
                                && !text.chars().any(char::is_control)
                        })
                        .ok_or_else(reject)?;
                    Ok(json!(selector))
                })
                .collect::<Result<_, PresetError>>()?;
            feature_values.insert(format!("{FALLBACK_PREFIX}{role}"), Value::Array(selectors));
        }
    }
    let preset = Preset {
        name: name.to_owned(),
        model,
        thinking_option_id: thinking.map(str::to_owned),
        feature_values,
    };
    Ok(preset)
}

/// Preserve retired and future selectors so one unresolved entry never locks the whole store.
fn model_route<'a>(catalog: &Catalog, selector: &'a str) -> (String, Option<&'a str>) {
    let known = catalog
        .models
        .iter()
        .filter(|model| selected(model, selector))
        .max_by_key(|model| model.selector.len());
    let (route, effort) = match known {
        Some(model) => (
            &selector[..model.selector.len()],
            selector
                .strip_prefix(&model.selector)
                .and_then(|suffix| suffix.strip_prefix(':')),
        ),
        None => match selector.rsplit_once(':') {
            Some((route, level)) if config::thinking_level(level).is_some() => (route, Some(level)),
            _ => (selector, None),
        },
    };
    let model = route.split_once('/').map_or_else(
        || route.to_owned(),
        |(provider, id)| json!([provider, id]).to_string(),
    );
    (model, effort)
}

fn resolve(catalog: &Catalog, roles: &Map<String, Value>, role: &str) -> Option<String> {
    let mut role = role;
    for _ in 0..=super::MAX_ROLES {
        let selector = match roles.get(role) {
            Some(value) => value.as_str()?,
            None => catalog.inherited.get(role)?.as_str(),
        };
        match selector.strip_prefix('@') {
            Some(alias) => role = alias,
            None => return Some(selector.to_owned()),
        }
    }
    None
}

fn definition(
    catalog: &Catalog,
    preset: &Preset,
    original: Option<&Value>,
) -> Result<Value, PresetError> {
    let previous = original.and_then(|raw| record(catalog, &preset.name, raw).ok());
    let same_model = previous
        .as_ref()
        .is_some_and(|old| old.model == preset.model);
    let route = config::model_argument(&preset.model).map_err(|_| {
        PresetError::Rejected(
            "OMP preset models must be native [provider, model] routes".to_owned(),
        )
    });
    let route = match route {
        Ok(route) => route,
        Err(_) if same_model => original
            .and_then(|raw| raw["modelRoles"]["default"].as_str())
            .unwrap_or(&preset.model)
            .to_owned(),
        Err(error) => return Err(error),
    };
    if !same_model
        && !catalog
            .models
            .iter()
            .any(|model| model.selector == route && model.kind == "chat")
    {
        return Err(PresetError::Rejected(
            "The preset's default model is not available in the OMP catalog".to_owned(),
        ));
    }
    if preset
        .thinking_option_id
        .as_deref()
        .is_some_and(|level| config::thinking_level(level).is_none())
        && previous
            .as_ref()
            .is_none_or(|old| old.thinking_option_id != preset.thinking_option_id)
    {
        return Err(PresetError::Rejected(
            "OMP does not support the preset's thinking level".to_owned(),
        ));
    }
    let (mut roles, fallbacks) =
        feature_roles_and_fallbacks(catalog, preset, original, previous.as_ref(), &route)?;
    roles.insert("default".to_owned(), json!(route));
    let mut value = original
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    value["modelRoles"] = json!(roles);
    if !fallbacks.is_empty() {
        value["fallbackChains"] = json!(fallbacks);
    } else if let Some(fields) = value.as_object_mut() {
        fields.remove("fallbackChains");
    }
    if let Some(thinking) = &preset.thinking_option_id {
        value["defaultThinkingLevel"] = json!(thinking);
    } else if let Some(fields) = value.as_object_mut() {
        fields.remove("defaultThinkingLevel");
    }
    Ok(value)
}

type RoleAndFallbackMaps = (Map<String, Value>, Map<String, Value>);

fn feature_roles_and_fallbacks(
    catalog: &Catalog,
    preset: &Preset,
    original: Option<&Value>,
    previous: Option<&Preset>,
    route: &str,
) -> Result<RoleAndFallbackMaps, PresetError> {
    let mut roles = Map::new();
    let mut fallbacks = Map::new();
    for (key, value) in &preset.feature_values {
        if let Some(role) = key.strip_prefix(FALLBACK_PREFIX) {
            if value.as_str() == Some(INHERIT) {
                continue;
            }
            let unchanged = previous.is_some_and(|old| old.feature_values.get(key) == Some(value));
            if !unchanged {
                let selection = domain::agent_runtime::StoredAgentConfig {
                    feature_values: Some(BTreeMap::from([(key.clone(), value.clone())])),
                    ..Default::default()
                };
                catalog.validate(&selection).map_err(|_| {
                    PresetError::Rejected(
                        "A preset fallback role or model is not available in the OMP catalog"
                            .to_owned(),
                    )
                })?;
            }
            fallbacks.insert(role.to_owned(), value.clone());
            continue;
        }
        let role = key.strip_prefix(PREFIX).ok_or_else(|| {
            PresetError::Rejected(
                "OMP native presets store model-role and fallback assignments only".to_owned(),
            )
        })?;
        let unchanged = previous.is_some_and(|old| old.feature_values.get(key) == Some(value));
        if unchanged && !value.is_string() {
            roles.insert(role.to_owned(), value.clone());
            continue;
        }
        let selector = value.as_str().ok_or_else(|| {
            PresetError::Rejected("OMP preset role assignments must be text".to_owned())
        })?;
        if !unchanged {
            let selection = domain::agent_runtime::StoredAgentConfig {
                feature_values: Some(BTreeMap::from([(key.clone(), value.clone())])),
                ..Default::default()
            };
            catalog.validate(&selection).map_err(|_| {
                PresetError::Rejected(
                    "A preset role or model is not available in the OMP catalog".to_owned(),
                )
            })?;
        }
        if role == "default" {
            if selector != route && selector != INHERIT {
                return Err(PresetError::Rejected(
                    "The default role conflicts with the preset's main model".to_owned(),
                ));
            }
        } else if selector != INHERIT {
            roles.insert(
                role.to_owned(),
                if selector == UNSET {
                    Value::Null
                } else if unchanged {
                    original
                        .and_then(|raw| raw["modelRoles"].get(role))
                        .unwrap_or(value)
                        .clone()
                } else {
                    value.clone()
                },
            );
        }
    }
    Ok((roles, fallbacks))
}

#[cfg(test)]
mod tests;
