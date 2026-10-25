//! Map OMP's ACP configuration to the existing controls and opaque model identities.

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use crate::local::{acp_config, acp_transport::Transport};
use crate::ports::agent_session::AgentSessionError;
use crate::protocol::provider::Details;

/// Apply advertised controls. Disposable probes never change native feature settings.
/// Unknown selections reject before submission; failed requests are not replayed.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    config: &StoredAgentConfig,
    ephemeral: bool,
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
        ("mode", config.mode_id.as_deref()),
    ] {
        let Some(selected) = selected else { continue };
        let option = acp_config::option(options, category).ok_or(AgentSessionError::Rejected)?;
        if !acp_config::choices_with_limit(option, 8192)?
            .iter()
            .any(|choice| choice["value"] == selected)
        {
            return Err(AgentSessionError::Rejected);
        }
        let config_id = acp_config::text(option, "id")?.to_owned();
        set(transport, id, options, &config_id, &json!(selected)).await?;
    }
    if !ephemeral {
        for (key, value) in config
            .feature_values
            .iter()
            .flatten()
            .filter(|(key, _)| *key != "transport")
        {
            set(transport, id, options, key, value).await?;
        }
    }
    Ok(())
}

async fn set(
    transport: &mut Transport,
    session: &str,
    options: &mut Value,
    key: &str,
    value: &Value,
) -> Result<(), AgentSessionError> {
    let option = options
        .as_array()
        .and_then(|options| options.iter().find(|option| option["id"] == key))
        .ok_or(AgentSessionError::Rejected)?;
    if option["currentValue"] == *value {
        return Ok(());
    }
    let result = transport
        .request(
            "session/set_config_option",
            json!({"sessionId":session,"configId":key,"value":value}),
        )
        .await?;
    *options = acp_config::state_with_limit(&result, 8192)?;
    if options.as_array().is_none_or(|options| {
        !options
            .iter()
            .any(|option| option["id"] == key && option["currentValue"] == *value)
    }) {
        return Err(AgentSessionError::Failed);
    }
    Ok(())
}

/// Describe native modes while retaining the permission and unattended semantics of RPC.
pub(super) fn modes(options: &Value) -> Vec<Value> {
    let known = super::native_config::modes();
    acp_config::option(options, "mode").and_then(|option| acp_config::choices_with_limit(option, 8192).ok())
        .unwrap_or_default().into_iter().map(|choice| {
            known.iter().find(|mode| mode["id"] == choice["value"]).cloned().unwrap_or_else(||
                json!({"id":choice["value"],"label":choice["name"],"description":choice["description"],"icon":"Bot"}))
        }).collect()
}

/// Expose every supported native feature with its actual value or a pending explicit override.
pub(super) fn features(options: &Value, requested: &StoredAgentConfig) -> Vec<Value> {
    let mut config = requested.clone();
    let values = config.feature_values.get_or_insert_default();
    for option in options
        .as_array()
        .into_iter()
        .flatten()
        .filter(|option| option["type"] == "boolean")
    {
        if let Some(id) = option["id"].as_str() {
            values
                .entry(id.to_owned())
                .or_insert_with(|| option["currentValue"].clone());
        }
    }
    let supported = |key: &str| {
        options.as_array().is_some_and(|options| {
            options.iter().any(|option| {
                option["id"] == key
                    && option["type"] == "boolean"
                    && option["currentValue"].is_boolean()
            })
        })
    };
    super::native_config::features(
        &config,
        super::native_config::ServiceTiers {
            fast: supported("fastMode"),
            slow: supported("slowMode"),
        },
    )
    .into_iter()
    .filter(|feature| supported(feature["id"].as_str().unwrap_or_default()))
    .collect()
}

/// Discover each model's own thinking choices without switching models or persisting settings.
/// Malformed native model metadata rejects rather than inventing capabilities.
pub(super) fn details(options: &Value) -> Result<Details, AgentSessionError> {
    let model_option = acp_config::option(options, "model").ok_or(AgentSessionError::Failed)?;
    let mut models = Vec::new();
    for choice in acp_config::choices_with_limit(model_option, 8192)? {
        let model = &choice["_meta"]["model"];
        let provider = super::native_config::text(model, "provider")?;
        let id = super::native_config::text(model, "id")?;
        if choice["value"] != format!("{provider}/{id}") {
            return Err(AgentSessionError::Failed);
        }
        let thinking = if choice["value"] == model_option["currentValue"] {
            acp_config::choices(
                acp_config::option(options, "thought_level").ok_or(AgentSessionError::Failed)?,
            )?
            .iter()
            .map(|choice| json!({"id":choice["value"],"label":choice["name"]}))
            .collect()
        } else {
            super::native_config::model_thinking_options(model)?
        };
        models.push(json!({"provider":"omp","id":json!([provider,id]).to_string(),
            "label":choice["name"],"isSelectable":true,"isDefault":choice["value"]==model_option["currentValue"],"thinkingOptions":thinking}));
    }
    Ok(Details {
        models,
        modes: modes(options),
        features: features(options, &StoredAgentConfig::default()),
    })
}

/// Derive the selected model's context window without disclosing native configuration to clients.
pub(super) fn context(options: &Value) -> Value {
    let model = acp_config::option(options, "model").and_then(|option| {
        acp_config::choices_with_limit(option, 8192)
            .ok()
            .and_then(|choices| {
                choices
                    .into_iter()
                    .find(|choice| choice["value"] == option["currentValue"])
            })
    });
    json!({"model":model.map(|model| &model["_meta"]["model"])})
}

/// Project native selections while retaining opaque OMP model IDs.
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
