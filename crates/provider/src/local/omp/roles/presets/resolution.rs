//! Resolve through OMP's public extension facade; startup fallback is not a preset success.

use std::io::Write;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::super::super::{OmpClient, session};

const COMMAND: &str = "ait-read-model-preset-default";
// ABI glue only: OMP owns aliases, fuzzy matching, routing suffixes and authentication.
const EXTENSION: &str = r#"
export default function(pi) {
  pi.registerCommand("ait-read-model-preset-default", {
    description: "Read the configured default model without inference or writes",
    handler: async (_args, ctx) => {
      const model = ctx.models.resolve("@default");
      ctx.ui.notify(JSON.stringify({aitPresetDefault: model ? [model.provider, model.id] : null}), "info");
    }
  });
}
"#;

/// Return the native model and effort only after any explicit default resolves without fallback.
/// No prompt is admitted unless the installed public read-only helper is in the native catalog.
pub(super) async fn read(
    client: &OmpClient,
    cwd: &str,
    config: &StoredAgentConfig,
    explicit: bool,
) -> Result<Value, String> {
    let helper = if explicit {
        Some(
            tokio::task::spawn_blocking(|| {
                let mut file = tempfile::Builder::new()
                    .prefix("ait-preset-read-")
                    .suffix(".js")
                    .tempfile()?;
                file.write_all(EXTENSION.as_bytes())?;
                file.flush()?;
                Ok::<_, std::io::Error>(file)
            })
            .await
            .map_err(|_| "Preset reader could not initialize")?
            .map_err(|_| "Preset reader could not initialize")?,
        )
    } else {
        None
    };
    let mut probe = session::probe_config_with_extension(
        client,
        cwd,
        None,
        Some(config),
        helper.as_ref().map(tempfile::NamedTempFile::path),
    )
    .await
    .map_err(|_| "Preset model could not be resolved by OMP")?;
    let result = async {
        let state = probe
            .request("get_state", json!({}))
            .await
            .map_err(|_| "OMP could not read the preset model")?;
        if !explicit {
            return Ok(state);
        }
        let commands = probe
            .request("get_available_commands", json!({}))
            .await
            .map_err(|_| "OMP preset resolver is unavailable")?;
        if !commands["commands"]
            .as_array()
            .is_some_and(|commands| commands.iter().any(|command| command["name"] == COMMAND))
        {
            return Err("OMP preset resolver is unavailable");
        }
        let events = probe
            .local_command(&format!("/{COMMAND}"))
            .await
            .map_err(|_| "OMP preset resolution failed")?;
        let mut resolved = None;
        for event in events {
            if event["type"] == "extension_ui_request"
                && event["method"] == "notify"
                && let Some(text) = event["message"].as_str()
                && let Ok(value) = serde_json::from_str::<Value>(text)
                && value.get("aitPresetDefault").is_some()
            {
                resolved = Some(value["aitPresetDefault"].clone());
            }
        }
        if resolved.as_ref() != Some(&json!([state["model"]["provider"], state["model"]["id"]])) {
            return Err(
                "Preset default model is unavailable or shadowed by another native setting",
            );
        }
        Ok(state)
    }
    .await;
    let closed = probe.close().await;
    let state = result.map_err(str::to_owned)?;
    closed.map_err(|_| "OMP preset reader could not close")?;
    Ok(state)
}
