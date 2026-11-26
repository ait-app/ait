//! Native configuration acknowledgement, with explicit unsupported-method classification.

use provider_core::ports::{
    agent_session::AgentSessionError,
    diagnostics::{Category, Diagnostic},
};
use provider_native::{acp_config, acp_transport::Transport};
use serde_json::{Value, json};

use super::OpenError;
use crate::discovery::Selection;

/// Exchange one control message without losing native error codes or retaining startup chatter.
pub(super) async fn request(
    transport: &mut Transport,
    method: &str,
    params: Value,
) -> Result<Value, OpenError> {
    let deadline = transport.control_deadline();
    let result = tokio::time::timeout(deadline, async {
        let id = transport.begin(method, params).await?;
        for _ in 0..128 {
            let response = transport.receive().await?;
            if response.get("method").is_none() && response["id"] == id {
                if let Some(error) = response.get("error") {
                    transport
                        .diagnostics
                        .observe_native(error, "rpc", "control");
                    if error["code"] == -32601 && response.get("result").is_none() {
                        return Err(OpenError::Unsupported);
                    }
                }
                return provider_native::acp_transport::response(&response).map_err(Into::into);
            }
            if response["method"].is_string() {
                if response.get("id").is_some() {
                    transport
                        .send(&json!({"jsonrpc":"2.0","id":response["id"],
                        "error":{"code":-32601,"message":"Unsupported client method"}}))
                        .await?;
                }
            } else {
                return Err(AgentSessionError::Failed.into());
            }
        }
        Err(AgentSessionError::Failed.into())
    })
    .await;
    let result = result.unwrap_or_else(|_| {
        transport
            .diagnostics
            .observe(Diagnostic::new(Category::Timeout, "acp"), method);
        Err(OpenError::Session(AgentSessionError::Failed))
    });
    result.map_err(|error| match error {
        OpenError::Session(error) => OpenError::Diagnosed {
            error,
            diagnostic: transport
                .diagnostics
                .failure()
                .unwrap_or_else(|| Diagnostic::new(Category::Protocol, "acp")),
        },
        error => error,
    })
}

/// Apply model, then its offered effort, then mode; retain only confirmed native values.
pub(super) async fn apply(
    transport: &mut Transport,
    id: &str,
    options: &mut Value,
    selection: &Selection,
    mode: Option<&str>,
) -> Result<(), OpenError> {
    let mut expected = Vec::new();
    for (category, value) in [
        ("model", selection.model.as_deref()),
        ("thought_level", selection.effort.as_deref()),
        (
            "mode",
            Some(match mode {
                Some("full-access") | None => "default",
                Some(mode) => mode,
            }),
        ),
    ] {
        let Some(value) = value else {
            if let Some(current) = acp_config::option(options, category)
                .and_then(|option| option["currentValue"].as_str())
            {
                expected.push((category, current.to_owned()));
            }
            continue;
        };
        expected.push((category, value.to_owned()));
        let option = acp_config::option(options, category).ok_or(OpenError::Unsupported)?;
        if !acp_config::choices(option)?
            .iter()
            .any(|choice| choice["value"] == value)
        {
            return Err(OpenError::Unsupported);
        }
        if option["currentValue"] == value {
            continue;
        }
        let response = request(
            transport,
            "session/set_config_option",
            json!({
                "sessionId":id,"configId":acp_config::text(option,"id")?,"value":value
            }),
        )
        .await?;
        let confirmed = acp_config::state(&response)?;
        if acp_config::option(&confirmed, category)
            .is_none_or(|option| option["currentValue"] != value)
        {
            return Err(AgentSessionError::Failed.into());
        }
        *options = confirmed;
    }
    // A later option can change earlier selections. Confirm the entire final snapshot,
    // including the native defaults retained when the host did not specify a value.
    if expected.iter().any(|(category, value)| {
        acp_config::option(options, category).is_none_or(|option| option["currentValue"] != *value)
    }) {
        return Err(AgentSessionError::Failed.into());
    }
    Ok(())
}
