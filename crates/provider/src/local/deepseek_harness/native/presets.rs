//! Native plugin compositions are distinct from tool permission presets.
use serde_json::{Value, json};

use super::super::config::text;
use crate::ports::agent_session::AgentSessionError;

pub(super) const PREFIX: &str = "agent-preset:";
pub(super) const PERMISSION: &str = "permission_preset";

/// Project the installed roster without inventing built-in modes or accepting broken compositions.
/// Invalid roster shapes return a protocol error; IDs remain opaque native identities.
pub(super) fn modes(roster: &Value) -> Result<Vec<Value>, AgentSessionError> {
    roster["presets"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .filter(|row| row.get("broken").is_none())
        .map(|row| {
            let id = text(row, "id")?;
            Ok(json!({"id":format!("{PREFIX}{id}"),
                "label":row["name"].as_str().unwrap_or(id),
                "description":row["description"].as_str().unwrap_or(""),
                "icon":"Bot", "isDefault":row["isDefault"] == true}))
        })
        .collect()
}

/// Build the separate permission selector from the Host's advertised catalog.
/// Reject malformed catalogs rather than supplying broader fallback permissions.
pub(super) fn permission_feature(permissions: &Value) -> Result<Value, AgentSessionError> {
    let options = permissions["options"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .filter(|row| row["value"] != "custom")
        .map(|row| Ok(json!({"id":text(row,"value")?,"label":text(row,"name")?})))
        .collect::<Result<Vec<_>, AgentSessionError>>()?;
    Ok(
        json!({"id":PERMISSION,"type":"select","label":"Permissions",
        "icon":"shield-check","value":permissions["currentValue"],"options":options}),
    )
}

/// Check draft selections against discovery without activating a plugin composition.
/// Returns rejection for unavailable models, reasoning efforts, modes or permission choices.
pub(in crate::local::deepseek_harness) fn validate_selection(
    details: &crate::protocol::provider::Details,
    config: &domain::agent_runtime::StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    super::config::validate(config)?;
    if let Some(mode) = config
        .mode_id
        .as_deref()
        .filter(|id| id.starts_with(PREFIX))
        && !details.modes.iter().any(|option| option["id"] == mode)
    {
        return Err(AgentSessionError::Rejected);
    }
    let permission = config
        .feature_values
        .as_ref()
        .and_then(|values| values.get(PERMISSION))
        .and_then(Value::as_str)
        .or_else(|| {
            config
                .mode_id
                .as_deref()
                .filter(|id| !id.starts_with(PREFIX))
        });
    if permission.is_some_and(|id| {
        !details.features.iter().any(|feature| {
            feature["id"] == PERMISSION
                && feature["options"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(|option| option["id"] == id))
        })
    }) {
        return Err(AgentSessionError::Rejected);
    }
    let model = details.models.iter().find(|model| {
        config
            .model
            .as_ref()
            .map_or(model["isDefault"] == true, |id| model["id"] == *id)
    });
    if config.model.is_some() && model.is_none() {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(effort) = config
        .thinking_option_id
        .as_deref()
        .filter(|id| !id.is_empty())
        && !model.is_some_and(|model| {
            model["thinkingOptions"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["id"] == effort))
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(())
}
