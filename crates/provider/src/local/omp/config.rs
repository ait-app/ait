use std::path::Path;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::{PROVIDER, transport::Transport};
use crate::ports::agent_session::RejectedConfigField;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use crate::protocol::provider::Details;

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "full" | "write" | "ask" | "ask-cached"))
        || config
            .provider_options
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|value| value.len() > 65536 || value.contains('\0'))
        || config.thinking_option_id.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
        || config.feature_values.iter().flatten().any(|(key, value)| {
            if key == "transport" {
                return !matches!(value.as_str(), Some("acp" | "rpc"));
            }
            if super::roles::is_key(key) {
                return !super::roles::valid(key, value);
            }
            !matches!(
                key.as_str(),
                "autoCompaction" | "autoRetry" | "fastMode" | "slowMode"
            ) || !value.is_boolean()
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(model) = &config.model {
        model_route(model)?;
    }
    Ok(())
}

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

pub(super) fn model_route(id: &str) -> Result<[String; 2], AgentSessionError> {
    let route: [String; 2] = serde_json::from_str(id).map_err(|_| AgentSessionError::Rejected)?;
    if route
        .iter()
        .any(|part| part.is_empty() || part.len() > 1024 || part.chars().any(char::is_control))
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(route)
}

/// Native service-tier support for one selected model, discovered in a disposable session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ServiceTiers {
    fast: bool,
    slow: bool,
}

/// Return toggles for `config`, exposing only tiers confirmed by native `tiers` discovery.
pub(super) fn features(config: &StoredAgentConfig, tiers: ServiceTiers) -> Vec<Value> {
    [
        ("autoCompaction", "Automatic compaction"),
        ("autoRetry", "Automatic retry"),
        ("fastMode", "Fast service tier"),
        ("slowMode", "Slow service tier"),
    ]
    .into_iter()
    .filter(|(id, _)| match *id {
        "fastMode" => tiers.fast,
        "slowMode" => tiers.slow,
        _ => true,
    })
    .map(|(id, label)| {
        json!({"type":"toggle","id":id,"label":label,
            "value":config.feature_values.as_ref().and_then(|values|values.get(id))
                .and_then(Value::as_bool).unwrap_or(matches!(id,"autoCompaction"|"autoRetry"))})
    })
    .collect()
}

/// Inspect service tiers using a no-session transport that will be discarded afterward.
/// Returns transport failures; a native rejection means Fast is unsupported.
pub(super) async fn service_tiers(
    transport: &mut Transport,
) -> Result<ServiceTiers, AgentSessionError> {
    let state = transport.request("get_state", json!({})).await?;
    // OMP exposes slowModeSupported, but Fast has no equivalent read-only RPC fact.
    // Fast changes only this disposable session's tier map; Slow can persist global settings.
    let fast = match transport
        .request("set_fast_mode", json!({"enabled":true}))
        .await
    {
        Ok(_) => true,
        Err(AgentSessionError::Rejected) => false,
        Err(error) => return Err(error),
    };
    Ok(ServiceTiers {
        fast,
        slow: state["slowModeSupported"] == true,
    })
}

/// Per-field progress of one live configuration application.
///
/// Acknowledged facts are captured as each native acknowledgement arrives, so a later
/// transport failure still persists exactly what the native session confirmed — never the
/// unacknowledged remainder of the proposal.
#[derive(Default)]
pub(super) struct LiveApply {
    /// Model route confirmed by the native `set_model` acknowledgement, if any.
    pub(super) acked_model: Option<String>,
    /// Level confirmed by the native `set_thinking_level` acknowledgement.
    pub(super) acked_thinking: Option<String>,
    /// Feature toggles the native session acknowledged, by feature id.
    pub(super) acked_features: std::collections::BTreeMap<String, Value>,
    /// Fields the native session refused while remaining usable.
    pub(super) rejected: Vec<RejectedConfigField>,
    /// Native state read back after every write succeeded.
    pub(super) state: Option<Value>,
}

/// Apply each proposed field to the native session, capturing acknowledgements as they arrive.
///
/// `current` is the session's last confirmed configuration. On success the progress carries the
/// final native readback; on failure it carries every fact acknowledged before the failing
/// request, and the caller commits only those. Absent overrides are not written: clearing an
/// override restores native inheritance on reopen.
pub(super) async fn apply(
    transport: &mut Transport,
    current: &StoredAgentConfig,
    proposed: &StoredAgentConfig,
    progress: &mut LiveApply,
) -> Result<(), AgentSessionError> {
    validate(proposed)?;
    // A cleared model override restores native inheritance on reopen; only a set model is
    // written live.
    if proposed.model != current.model
        && let Some(model) = &proposed.model
    {
        let [provider, model] = model_route(model)?;
        match transport
            .request("set_model", json!({"provider":provider,"modelId":model}))
            .await
        {
            Ok(acked) => {
                // Official OMP answers set_model with the resolved model object; the
                // acknowledged selection is that native fact, not the requested input.
                let actual_provider = text(&acked, "provider")?;
                let actual_id = text(&acked, "id")?;
                progress.acked_model = Some(json!([actual_provider, actual_id]).to_string());
            }
            Err(AgentSessionError::Rejected) => {
                progress.rejected.push(RejectedConfigField {
                    field: "modelId".to_owned(),
                });
            }
            Err(error) => return Err(error),
        }
    }
    if proposed.thinking_option_id != current.thinking_option_id
        && let Some(level) = &proposed.thinking_option_id
    {
        let levels = transport
            .request("get_available_thinking_levels", json!({}))
            .await?;
        let supported = levels["levels"]
            .as_array()
            .is_some_and(|levels| levels.iter().any(|item| item == level));
        if supported {
            match transport
                .request("set_thinking_level", json!({"level":level}))
                .await
            {
                // Official OMP answers set_thinking_level with a bare success ack after
                // assigning the level verbatim (no clamping); the requested level is the
                // confirmed fact. The final get_state readback re-verifies it on success.
                Ok(_) => progress.acked_thinking = Some(level.clone()),
                Err(AgentSessionError::Rejected) => {
                    progress.rejected.push(RejectedConfigField {
                        field: "thinkingOptionId".to_owned(),
                    });
                }
                Err(error) => return Err(error),
            }
        } else {
            progress.rejected.push(RejectedConfigField {
                field: "thinkingOptionId".to_owned(),
            });
        }
    }
    let current_features = current.feature_values.as_ref();
    for (id, value) in proposed.feature_values.iter().flatten() {
        if current_features.and_then(|values| values.get(id)) == Some(value) {
            continue;
        }
        let kind = match id.as_str() {
            "transport" => continue,
            // Role assignments never reach a live `set_*` toggle: they are applied by the private
            // `--config` overlay the native process read at startup, and `Session::apply_config`
            // reopens whenever the overlay changes. Reaching this loop means the overlay is
            // already in effect, so the stored value is confirmed rather than dropped.
            id if super::roles::is_key(id) => {
                progress.acked_features.insert(id.to_owned(), value.clone());
                continue;
            }
            "autoCompaction" => "set_auto_compaction",
            "autoRetry" => "set_auto_retry",
            "fastMode" => "set_fast_mode",
            "slowMode" => "set_slow_mode",
            _ => {
                progress.rejected.push(RejectedConfigField {
                    field: format!("featureValues.{id}"),
                });
                continue;
            }
        };
        match transport.request(kind, json!({"enabled":value})).await {
            Ok(_) => {
                progress.acked_features.insert(id.clone(), value.clone());
            }
            Err(AgentSessionError::Rejected) => {
                progress.rejected.push(RejectedConfigField {
                    field: format!("featureValues.{id}"),
                });
            }
            Err(error) => return Err(error),
        }
    }
    progress.state = Some(transport.request("get_state", json!({})).await?);
    Ok(())
}

/// Commit only acknowledged facts onto the prior configuration.
///
/// Used when the application failed after some acknowledgements: unacknowledged fields keep
/// their prior values, and nothing is invented from the proposal.
pub(super) fn acknowledged(current: &StoredAgentConfig, progress: &LiveApply) -> StoredAgentConfig {
    let mut next = current.clone();
    if let Some(model) = &progress.acked_model {
        next.model = Some(model.clone());
    }
    if let Some(level) = &progress.acked_thinking {
        next.thinking_option_id = Some(level.clone());
    }
    if !progress.acked_features.is_empty() {
        let values = next.feature_values.get_or_insert_with(Default::default);
        for (id, value) in &progress.acked_features {
            values.insert(id.clone(), value.clone());
        }
    }
    next
}

/// Reconcile a fully acknowledged application with the native state read back afterwards.
///
/// Acknowledged model/thinking values are replaced by the observed native facts, so a plugin
/// that rewrote the selection is persisted truthfully. Rejected fields keep their prior values;
/// fields that were never written keep them too. Feature toggles have no readback fact;
/// acknowledged values are retained as proposed.
pub(super) fn confirmed(
    current: &StoredAgentConfig,
    proposed: &StoredAgentConfig,
    state: &Value,
    progress: &LiveApply,
) -> Result<StoredAgentConfig, AgentSessionError> {
    let observed = runtime(state)?;
    let mut next = current.clone();
    let refused = |field: &str| progress.rejected.iter().any(|entry| entry.field == field);
    if proposed.model != current.model && !refused("modelId") && progress.acked_model.is_some() {
        next.model.clone_from(&observed.model);
    }
    if proposed.thinking_option_id != current.thinking_option_id
        && !refused("thinkingOptionId")
        && progress.acked_thinking.is_some()
    {
        next.thinking_option_id
            .clone_from(&observed.thinking_option_id);
    }
    if let Some(proposed_values) = &proposed.feature_values {
        let values = next.feature_values.get_or_insert_with(Default::default);
        for (id, value) in proposed_values {
            if progress.acked_features.contains_key(id) {
                values.insert(id.clone(), value.clone());
            }
        }
    }
    validate(&next)?;
    Ok(next)
}

pub(super) async fn discover(transport: &mut Transport) -> Result<Details, AgentSessionError> {
    let state = transport.request("get_state", json!({})).await?;
    let models = transport.request("get_available_models", json!({})).await?;
    let models = models["models"]
        .as_array()
        .filter(|models| models.len() <= 8192)
        .ok_or(AgentSessionError::Failed)?;
    let available = transport
        .request("get_available_thinking_levels", json!({}))
        .await?;
    let levels = available["levels"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut entries = Vec::with_capacity(models.len());
    for model in models {
        let provider = text(model, "provider")?;
        let id = text(model, "id")?;
        let thinking: Vec<Value> =
            if state["model"]["provider"] == provider && state["model"]["id"] == id {
                levels
                    .iter()
                    .map(|level| json!({"id":level,"label":level}))
                    .collect()
            } else {
                model_thinking_options(model)?
            };
        entries.push(
            json!({"provider":PROVIDER,"id":json!([provider,id]).to_string(),
            "label":model["name"].as_str().unwrap_or(id),"isSelectable":true,
            "isDefault":state["model"]["provider"] == provider && state["model"]["id"] == id,
            "thinkingOptions":thinking}),
        );
    }
    Ok(Details {
        models: entries,
        modes: modes(),
        features: features(
            &StoredAgentConfig::default(),
            service_tiers(transport).await?,
        ),
    })
}

fn model_thinking_options(model: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let mut options = vec![json!({"id":"off","label":"off"})];
    if model["reasoning"] != true {
        return Ok(options);
    }
    let Some(efforts) = model["thinking"].get("efforts") else {
        return Ok(options);
    };
    let efforts = efforts
        .as_array()
        .filter(|efforts| efforts.len() <= 64)
        .ok_or(AgentSessionError::Failed)?;
    for effort in efforts {
        if !effort.as_str().is_some_and(|effort| {
            !effort.is_empty() && effort.len() <= 128 && !effort.chars().any(char::is_control)
        }) || options.iter().any(|option| option["id"] == *effort)
        {
            return Err(AgentSessionError::Failed);
        }
        options.push(json!({"id":effort,"label":effort}));
    }
    Ok(options)
}

pub(super) fn runtime(state: &Value) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
    let id = text(state, "sessionId")?;
    let model = state
        .get("model")
        .filter(|model| !model.is_null())
        .map(|model| Ok(json!([text(model, "provider")?, text(model, "id")?]).to_string()))
        .transpose()?;
    Ok(StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model,
        thinking_option_id: state["thinkingLevel"].as_str().map(str::to_owned),
        mode_id: None,
        extra: None,
    })
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

pub(super) fn commands(value: &Value) -> Result<Vec<Value>, AgentSessionError> {
    value["commands"]
        .as_array()
        .filter(|commands| commands.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .map(|entry| {
            Ok(
                json!({"name":text(entry,"name")?,"description":entry["description"].as_str()
                .unwrap_or_default(),"argumentHint":entry["input"]["hint"].as_str().unwrap_or_default(),
                "kind":if entry["source"]=="skill"{"skill"}else{"command"}}),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests;

pub(super) fn modes() -> Vec<Value> {
    [
        (
            "full",
            "Full Access",
            "Approves ordinary read, write and execution tools.",
            "dangerous",
        ),
        (
            "write",
            "Allow File Writes",
            "Allows reads and file writes; command execution requires approval.",
            "moderate",
        ),
        (
            "ask",
            "Always Ask",
            "Requires approval for write and execution tools.",
            "safe",
        ),
        (
            "ask-cached",
            "Ask and Remember",
            "Requires approval for ungranted files and execution tools.",
            "safe",
        ),
    ]
    .into_iter()
    .map(|(id, label, description, tier)| {
        json!({"id":id,"label":label,"description":description,
            "colorTier":tier,"isUnattended":id=="full"})
    })
    .collect()
}

pub(super) fn approval_mode(mode: &str) -> &str {
    match mode {
        "full" => "yolo",
        "ask" => "always-ask",
        "ask-cached" => "always-ask-cached",
        _ => "write",
    }
}
