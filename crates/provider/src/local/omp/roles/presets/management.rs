//! Explicit global preset management through OMP's official built-in commands.

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::super::super::{OmpClient, config, session};
use crate::ports::agent_session::{AgentClient, AgentSessionError, AgentSessionFuture};

/// Validate the bounded operation before constructing its independent native work future.
pub(in super::super) fn begin(
    client: OmpClient,
    cwd: String,
    config: StoredAgentConfig,
    action: &str,
    arguments: &Value,
) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
    let operation = match action {
        "model_preset_save" => "save",
        "model_preset_delete" => "delete",
        _ => return Err(AgentSessionError::Rejected),
    };
    let name = arguments["name"]
        .as_str()
        .filter(|name| super::valid_name(name))
        .ok_or(AgentSessionError::Rejected)?
        .to_owned();
    if arguments.as_object().is_none_or(|fields| fields.len() != 1) {
        return Err(AgentSessionError::Rejected);
    }
    Ok(Box::pin(async move {
        manage(&client, &cwd, config, operation, &name).await
    }))
}

async fn manage(
    client: &OmpClient,
    cwd: &str,
    mut selection: StoredAgentConfig,
    operation: &str,
    name: &str,
) -> Result<Value, AgentSessionError> {
    let metadata = client.metadata_gate(cwd)?.lock_owned().await;
    if operation == "delete" {
        let before = super::super::read(
            client,
            cwd,
            &["config", "list", "--scope", "global", "--json"],
        )
        .await?;
        if before["modelPresets"]["value"][name].is_null() {
            return Ok(
                json!({"text":"","error":"Preset is not defined globally; project presets must be removed in their project configuration"}),
            );
        }
    }
    let values = selection.feature_values.get_or_insert_default();
    if let Some(model) = &selection.model {
        let [provider, id] = config::model_route(model)?;
        let suffix = selection
            .thinking_option_id
            .as_ref()
            .map(|level| format!(":{level}"))
            .unwrap_or_default();
        values.insert(
            "modelRole.default".to_owned(),
            json!(format!("{provider}/{id}{suffix}")),
        );
    }
    if let Some(thinking) = &selection.thinking_option_id {
        values.insert("modelPresetThinking".to_owned(), json!(thinking));
    }
    let expected = if operation == "save" {
        let base = super::super::read(client, cwd, &["config", "list", "--json"]).await?;
        Some(saved_configuration(&base, &selection))
    } else {
        None
    };
    let mut native = session::probe_config(client, cwd, None, Some(&selection)).await?;
    let checked = async {
        let commands = native.request("get_available_commands", json!({})).await?;
        if !commands["commands"].as_array().is_some_and(|commands| {
            commands
                .iter()
                .any(|command| command["name"] == "modelpreset")
        }) {
            return Err(AgentSessionError::Unavailable);
        }
        let events = native
            .local_command(&format!("/modelpreset {operation} {name}"))
            .await?;
        let wrote = wait_for_write(client, cwd, name, expected.as_ref()).await?;
        let project = super::super::read(
            client,
            cwd,
            &["config", "list", "--scope", "project", "--json"],
        )
        .await?;
        let project_exists = !project["modelPresets"]["value"][name].is_null();
        let output: Vec<_> = events
            .into_iter()
            .filter(|event| event["type"] == "command_output")
            .filter_map(|event| event["text"].as_str().map(str::to_owned))
            .collect();
        Ok::<_, AgentSessionError>((
            output.join("\n"),
            wrote && (operation == "save" || !project_exists),
        ))
    }
    .await;
    let closed = native.close().await;
    let (text, completed) = checked?;
    closed?;
    drop(metadata);
    let refreshed = client.discover(cwd).await.is_ok();
    Ok(
        json!({"text":text,"error":if !completed {Some("OMP did not complete the requested preset change")} else if !refreshed {Some("Preset changed, but its model catalog could not refresh")} else {None}}),
    )
}

fn saved_configuration(base: &Value, selection: &StoredAgentConfig) -> Value {
    let mut roles: std::collections::BTreeMap<String, String> = base["modelRoles"]["value"]
        .as_object()
        .into_iter()
        .flat_map(serde_json::Map::iter)
        .filter_map(|(role, value)| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .map(|value| (role.clone(), value.to_owned()))
        })
        .collect();
    for (role, selector) in super::super::overrides(selection) {
        if selector == "unset" {
            roles.remove(&role);
        } else {
            roles.insert(role, selector);
        }
    }
    let thinking = selection
        .feature_values
        .as_ref()
        .and_then(|values| values.get("modelPresetThinking"))
        .unwrap_or(&base["defaultThinkingLevel"]["value"]);
    json!({"modelRoles":roles,"defaultThinkingLevel":thinking})
}

async fn wait_for_write(
    client: &OmpClient,
    cwd: &str,
    name: &str,
    expected: Option<&Value>,
) -> Result<bool, AgentSessionError> {
    let deadline = tokio::time::Instant::now() + client.deadline;
    loop {
        let global = super::super::read(
            client,
            cwd,
            &["config", "list", "--scope", "global", "--json"],
        )
        .await?;
        let actual = &global["modelPresets"]["value"][name];
        if expected.map_or_else(|| actual.is_null(), |expected| actual == expected) {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}
