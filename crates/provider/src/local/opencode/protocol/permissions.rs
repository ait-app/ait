//! Native permission fields and reply routes stay behind the protocol boundary.
use super::{Version, failure, http::Api};
use crate::local::opencode::types::{
    ApprovalRequest, ApprovalTarget, Decision, Fault, Invocation, ProtocolError,
};
use reqwest::Method;
use serde_json::{Value, json};

impl Api {
    pub(in crate::local::opencode) async fn pending_permissions(
        &self,
        session: &str,
    ) -> Result<Vec<Value>, ProtocolError> {
        let path = match self.version {
            Version::V1 => "/permission".to_owned(),
            Version::V2 => self.path(session, "/permission"),
        };
        let response = self.json(Method::GET, &path, None).await?;
        self.data(&response).as_array().cloned().ok_or_else(|| {
            failure(
                Fault::ProviderFailed,
                "invalid OpenCode pending permissions",
            )
        })
    }

    pub(in crate::local::opencode) fn approval(
        &self,
        request: &Invocation,
        session: &str,
        data: &Value,
    ) -> Option<ApprovalRequest> {
        normalize(self.version, request, session, data)
    }

    pub(in crate::local::opencode) async fn reply_permission(
        &self,
        session: &str,
        id: &str,
        decision: Decision,
    ) -> Result<(), ProtocolError> {
        let reply = match decision {
            Decision::Approved => "once",
            Decision::ApprovedAlways => "always",
            Decision::Denied | Decision::Cancelled => "reject",
        };
        let (path, body) = match self.version {
            Version::V1 => (format!("/permission/{id}/reply"), json!({"reply":reply})),
            Version::V2 => (
                self.path(session, &format!("/permission/{id}/reply")),
                json!({"decision":reply}),
            ),
        };
        self.json(Method::POST, &path, Some(&body)).await?;
        Ok(())
    }
}

pub(in crate::local::opencode) fn normalize(
    version: Version,
    request: &Invocation,
    _session: &str,
    data: &Value,
) -> Option<ApprovalRequest> {
    let id = data.get("id")?.as_str()?;
    let action = data
        .get(match version {
            Version::V1 => "permission",
            Version::V2 => "action",
        })?
        .as_str()?;
    let resources = data
        .get(match version {
            Version::V1 => "patterns",
            Version::V2 => "resources",
        })?
        .as_array()?;
    if action.is_empty()
        || action.len() > 256
        || action.chars().any(char::is_control)
        || resources.is_empty()
        || resources.len() > 64
        || resources.iter().any(|v| {
            v.as_str()
                .is_none_or(|v| v.is_empty() || v.len() > 4096 || v.contains('\0'))
        })
    {
        return None;
    }
    let native = || ApprovalTarget::Native {
        action: action.to_owned(),
        resources: resources
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
    };
    let target = match action {
        "bash" | "shell" if resources.len() == 1 => {
            let explicit = data.pointer("/metadata/command").and_then(Value::as_str);
            let command = explicit.or_else(|| resources[0].as_str())?;
            if command.len() > 4096 || command.contains('\0') {
                return None;
            }
            if explicit.is_none() && command.contains(['*', '?', '[', ']']) {
                native()
            } else {
                ApprovalTarget::Command {
                    command: command.to_owned(),
                    cwd: request.cwd.to_string_lossy().into_owned(),
                }
            }
        }
        "edit"
            if resources.iter().all(|value| {
                value
                    .as_str()
                    .is_some_and(|path| !path.contains(['*', '?', '[', ']']))
            }) =>
        {
            let paths = resources
                .iter()
                .filter_map(Value::as_str)
                .map(|path| {
                    let path = std::path::Path::new(path);
                    if path.is_absolute() {
                        path.to_owned()
                    } else {
                        request.cwd.join(path)
                    }
                    .to_string_lossy()
                    .into_owned()
                })
                .collect();
            ApprovalTarget::Files { paths }
        }
        _ => native(),
    };
    Some(ApprovalRequest {
        id: id.to_owned(),
        target,
        save_resources: data
            .get(match version {
                Version::V1 => "always",
                Version::V2 => "save",
            })
            .and_then(Value::as_array)
            .filter(|values| values.len() <= 64)
            .filter(|values| {
                values.iter().all(|v| {
                    v.as_str()
                        .is_some_and(|s| !s.is_empty() && s.len() <= 4096 && !s.contains('\0'))
                })
            })
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}
