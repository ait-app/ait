//! Map OMP's ACP configuration back to the existing OMP model identities.

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use crate::local::{acp_config, acp_transport::Transport};
use crate::ports::agent_session::AgentSessionError;

/// Apply model, effort and native ACP mode using only advertised choices.
/// Unknown selections reject; failed requests are not replayed.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    config: &StoredAgentConfig,
) -> Result<(), AgentSessionError> {
    let model = config
        .model
        .as_deref()
        .map(super::native_config::model_route)
        .transpose()?
        .map(|[provider, model]| format!("{provider}/{model}"));
    for (category, selected) in [
        ("model", model.as_deref()),
        ("thought_level", config.thinking_option_id.as_deref()),
        (
            "mode",
            config
                .mode_id
                .as_deref()
                .filter(|mode| matches!(*mode, "default" | "plan")),
        ),
    ] {
        let Some(selected) = selected else { continue };
        let option = acp_config::option(options, category).ok_or(AgentSessionError::Rejected)?;
        if !acp_config::choices(option)?
            .iter()
            .any(|choice| choice["value"] == selected)
        {
            return Err(AgentSessionError::Rejected);
        }
        if option["currentValue"] != selected {
            let result = transport
                .request(
                    "session/set_config_option",
                    json!({"sessionId":id,
                "configId":acp_config::text(option,"id")?,"value":selected}),
                )
                .await?;
            *options = acp_config::state(&result)?;
            if acp_config::option(options, category)
                .is_none_or(|option| option["currentValue"] != selected)
            {
                return Err(AgentSessionError::Failed);
            }
        }
    }
    Ok(())
}

/// Describe native ACP modes, keeping execution permissions in OMP's own permission gate.
pub(super) fn modes(options: &Value) -> Vec<Value> {
    acp_config::option(options,"mode").and_then(|option| acp_config::choices(option).ok()).unwrap_or_default()
        .into_iter().map(|choice| json!({"id":choice["value"],"label":choice["name"],
            "description":choice["description"],"icon":if choice["value"] == "plan" {"ShieldCheck"} else {"Bot"}})).collect()
}

/// Project native selections while retaining the RPC adapter's opaque OMP model IDs.
pub(super) fn runtime(id: &str, options: &Value) -> StoredAgentRuntimeInfo {
    let selection = |category| {
        acp_config::option(options, category)
            .and_then(|option| option["currentValue"].as_str())
            .map(str::to_owned)
    };
    let model = selection("model").and_then(|value| {
        value
            .split_once('/')
            .map(|(provider, model)| json!([provider, model]).to_string())
    });
    StoredAgentRuntimeInfo {
        provider: "omp".to_owned(),
        session_id: Some(id.to_owned()),
        model,
        thinking_option_id: selection("thought_level"),
        mode_id: selection("mode"),
        extra: None,
    }
}
