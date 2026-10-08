//! Session admission and durable reconciliation translate each native wire format.
use super::{
    Version, execution, failure, history,
    http::{Api, required_string},
};
use crate::local::opencode::{
    session::valid_id,
    types::{Fault, Invocation, ProtocolError, Snapshot},
};
use reqwest::Method;
use serde_json::{Value, json};

impl Api {
    pub(in crate::local::opencode) async fn prepare_session(
        &self,
        request: &Invocation,
    ) -> Result<Snapshot, ProtocolError> {
        let api = self;
        let id = if let Some(id) = &request.session_id {
            if !api.idle(id).await? {
                return Err(failure(
                    Fault::SessionBusy,
                    "OpenCode session is active elsewhere",
                ));
            }
            if api.version == Version::V2 {
                api.json(
                    Method::POST,
                    &api.path(id, "/model"),
                    Some(&json!({"model":model(request, api.version)})),
                )
                .await?;
                api.json(
                    Method::POST,
                    &api.path(id, "/agent"),
                    Some(&json!({"agent":request.agent})),
                )
                .await?;
            }
            id.clone()
        } else {
            let body = match api.version {
                Version::V1 => json!({}),
                Version::V2 => json!({"location":{"directory":request.cwd}, "agent":request.agent,
                "model":model(request, api.version)}),
            };
            let response = api
                .json(
                    Method::POST,
                    &format!("{}/session", api.version.prefix()),
                    Some(&body),
                )
                .await?;
            let id = required_string(api.data(&response), "id")?;
            if !valid_id(id) {
                return Err(failure(
                    Fault::ProviderFailed,
                    "invalid OpenCode session identity",
                ));
            }
            if api.version == Version::V2
                && let Some(instructions) = &request.instructions
            {
                let path = format!("/api/experimental/session/{id}/instructions/entries/ait");
                api.json(Method::PUT, &path, Some(&json!({"value":instructions})))
                    .await?;
            }
            id.to_owned()
        };
        let mut history = api.snapshot(&id, request).await?;
        if api.version == Version::V1 && !history.input_id.starts_with("msg_") {
            history.input_id = api.new_input_id();
        }
        Ok(history)
    }

    pub(in crate::local::opencode) async fn snapshot(
        &self,
        id: &str,
        request: &Invocation,
    ) -> Result<Snapshot, ProtocolError> {
        let api = self;
        if !api.idle(id).await? {
            return Err(failure(
                Fault::SessionBusy,
                "OpenCode execution is not idle",
            ));
        }
        let response = api.json(Method::GET, &api.path(id, ""), None).await?;
        let info = api.data(&response);
        let cwd = match api.version {
            Version::V1 => info.get("directory"),
            Version::V2 => info.pointer("/location/directory"),
        }
        .and_then(Value::as_str)
        .ok_or_else(|| failure(Fault::ProviderFailed, "OpenCode cwd missing"))?;
        if required_string(info, "id")? != id || std::path::Path::new(cwd) != request.cwd {
            return Err(failure(
                Fault::AgentCapabilityUnsupported,
                "OpenCode session identity or cwd differ from admission",
            ));
        }
        if request.verify_settings
            && api.version == Version::V2
            && (info.pointer("/model/providerID").and_then(Value::as_str)
                != request.model.split_once('/').map(|(provider, _)| provider)
                || info.pointer("/model/id").and_then(Value::as_str)
                    != request.model.split_once('/').map(|(_, model)| model)
                || request.reasoning_effort.as_deref().is_some_and(|effort| {
                    info.pointer("/model/variant").and_then(Value::as_str) != Some(effort)
                }))
        {
            return Err(failure(
                Fault::AgentCapabilityUnsupported,
                "OpenCode effective model differs from admission",
            ));
        }
        let execution = if api.version == Version::V2 {
            api.execution(id).await?
        } else {
            None
        };
        let raw = api.history(id).await?;
        let outcome = execution::outcome(api.version, info, execution.as_ref(), &raw)?;
        if api.version == Version::V2 && api.execution(id).await? != execution {
            return Err(failure(
                Fault::SessionBusy,
                "OpenCode execution changed during reconciliation",
            ));
        }
        if api.version == Version::V2 && execution.is_some() && outcome.is_none() {
            return Err(failure(
                Fault::SessionBusy,
                "OpenCode durable execution has not drained",
            ));
        }
        let messages = history::normalize(api.version, id, &raw)?;
        if !api.idle(id).await? {
            return Err(failure(
                Fault::SessionBusy,
                "OpenCode became active during history reconciliation",
            ));
        }
        Ok(Snapshot {
            driver: "opencode".into(),
            id: id.into(),
            input_id: request.input_id.clone(),
            cwd: request.cwd.clone(),
            model: request.model.clone(),
            reasoning_effort: request.reasoning_effort.clone(),
            messages,
            outcome,
        })
    }
}
fn model(request: &Invocation, version: Version) -> Value {
    let (provider, model) = request
        .model
        .split_once('/')
        .expect("validated provider/model identifier");
    version.model((provider, model), request.reasoning_effort.as_deref())
}
