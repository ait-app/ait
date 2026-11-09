//! The unified preset editor's view of OMP's native preset store.
//!
//! Reads and writes stay on the official surfaces already used by the model-preset feature:
//! `config list --json` for reading and the native `/modelpreset save|delete` command for
//! writing. Nothing here adds a second store or changes those commands' semantics.

use domain::agent_runtime::StoredAgentConfig;
use serde_json::Value;

use crate::local::omp::roles::presets;
use crate::local::omp::{OmpClient, roles, session};
use crate::ports::preset::{Preset, PresetBackend, PresetDescriptor, PresetEntry, PresetError};

/// Why the editor must stay read-only for this provider.
const UNAVAILABLE: &str =
    "This OMP version does not expose the modelpreset command; upgrade OMP to manage presets";

/// The native descriptor: create, edit and delete are the two official commands.
///
/// `managed` reports rename closed for a native store because the CLI has no rename operation;
/// renaming there means saving the new name and deleting the old one, which is not atomic and
/// could lose the preset on failure. This only adds the reason the editor displays.
pub(super) fn descriptor() -> PresetDescriptor {
    let mut descriptor = PresetDescriptor::managed(PresetBackend::Native);
    descriptor.reason =
        Some("OMP has no rename command; save the new name, then delete the old one".to_owned());
    descriptor
}

/// Report whether the native store is usable for this directory.
pub(in crate::local::omp) async fn describe(client: &OmpClient, cwd: &str) -> PresetDescriptor {
    if presets_available(client, cwd).await {
        descriptor()
    } else {
        PresetDescriptor::unavailable(PresetBackend::Native, UNAVAILABLE)
    }
}

/// Read the acknowledged native presets without changing them.
pub(in crate::local::omp) async fn list(
    client: &OmpClient,
    cwd: &str,
) -> Result<Vec<PresetEntry>, PresetError> {
    let cwd = probe_cwd(cwd);
    let catalog = roles::Catalog::read(client, &cwd)
        .await
        .map_err(|_| PresetError::Unavailable("OMP presets could not be read".to_owned()))?;
    Ok(catalog.preset_records())
}

/// Save through OMP's own command and require its acknowledged write.
///
/// The selection is expressed as the same private configuration a client applies: the model,
/// its thinking suffix and the explicit role overrides, never a global role mutation.
pub(in crate::local::omp) async fn save(
    client: &OmpClient,
    cwd: &str,
    preset: &Preset,
) -> Result<(), PresetError> {
    let selection = selection(preset)?;
    write(client, &probe_cwd(cwd), "save", &preset.name, selection).await
}

/// Delete through OMP's own command; project definitions are reported, never silently kept.
pub(in crate::local::omp) async fn delete(
    client: &OmpClient,
    cwd: &str,
    name: &str,
) -> Result<(), PresetError> {
    write(
        client,
        &probe_cwd(cwd),
        "delete",
        name,
        StoredAgentConfig::default(),
    )
    .await
}

fn selection(preset: &Preset) -> Result<StoredAgentConfig, PresetError> {
    let route = serde_json::from_str::<[String; 2]>(&preset.model)
        .ok()
        .filter(|route| {
            route.iter().all(|part| {
                !part.is_empty() && part.len() <= 1024 && !part.chars().any(char::is_control)
            })
        })
        .ok_or_else(|| {
            PresetError::Rejected(
                "OMP preset models must be a native [provider, model] route".to_owned(),
            )
        })?;
    let _ = route;
    let mut selection = StoredAgentConfig {
        model: Some(preset.model.clone()),
        thinking_option_id: preset.thinking_option_id.clone(),
        ..StoredAgentConfig::default()
    };
    if !preset.feature_values.is_empty() {
        selection.feature_values = Some(preset.feature_values.clone());
    }
    Ok(selection)
}

async fn write(
    client: &OmpClient,
    cwd: &str,
    operation: &str,
    name: &str,
    selection: StoredAgentConfig,
) -> Result<(), PresetError> {
    if !presets::valid_name(name) {
        return Err(PresetError::Rejected(
            "OMP preset names must start with a letter and contain only letters, digits, '-' or '_'".to_owned(),
        ));
    }
    let result = presets::management::begin(
        client.clone(),
        cwd.to_owned(),
        selection,
        if operation == "save" {
            "model_preset_save"
        } else {
            "model_preset_delete"
        },
        &serde_json::json!({"name":name}),
    )
    .map_err(|_| PresetError::Unavailable("OMP rejected the preset operation".to_owned()))?;
    let reply: Value = result.await.map_err(|_| {
        PresetError::Unavailable("OMP did not complete the preset operation".to_owned())
    })?;
    match reply["error"].as_str() {
        None => Ok(()),
        Some(error) => Err(PresetError::Rejected(error.to_owned())),
    }
}

/// Probe the native command directory from a directory OMP accepts.
///
/// A provider settings sheet has no project yet, so the caller passes no working directory; the
/// daemon's own directory is `/` under a launcher, where OMP refuses to start. Those cases fall
/// back to the home directory, which is the harness's own state root and always supports a
/// read-only command probe. A real project directory is preserved so project-scoped presets keep
/// their own scope.
///
/// The resolved directory is always absolute: a request without `cwd` reaches the daemon as the
/// relative `.`, which OMP's own spec validation rejects before the native process starts, so a
/// relative path that exists would otherwise report the store as unavailable.
pub(super) fn probe_cwd(cwd: &str) -> String {
    let canonical = std::path::Path::new(cwd)
        .canonicalize()
        .ok()
        .filter(|path| path != std::path::Path::new("/"));
    if let Some(canonical) = canonical {
        return canonical.to_string_lossy().into_owned();
    }
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_dir())
        .map_or_else(
            || cwd.to_owned(),
            |home| home.to_string_lossy().into_owned(),
        )
}

async fn presets_available(client: &OmpClient, cwd: &str) -> bool {
    let cwd = probe_cwd(cwd);
    match session::probe_config(client, &cwd, None, None).await {
        Ok(mut probe) => {
            let commands = probe
                .request("get_available_commands", serde_json::json!({}))
                .await;
            let available = commands.is_ok_and(|commands| {
                commands["commands"].as_array().is_some_and(|commands| {
                    commands
                        .iter()
                        .any(|command| command["name"] == "modelpreset")
                })
            });
            let _ = probe.close().await;
            available
        }
        Err(_) => false,
    }
}
