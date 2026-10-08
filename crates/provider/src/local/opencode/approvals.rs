//! Asynchronous native approvals retain exact request identity and retain native approval scope.
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::local::opencode::types::{ApprovalRequest, Decision, Invocation};
use crate::local::opencode::types::{ApprovalTarget, Fault, ProtocolError};
use reqwest::Method;
use serde_json::{Value, json};
use tokio_util::task::AbortOnDropHandle;

use super::{
    failure,
    http::{Api, Version, required_string},
    session::valid_id,
};

pub(super) struct Pending {
    tasks: HashMap<String, Task>,
    sender: tokio::sync::mpsc::Sender<Result<Resolution, ProtocolError>>,
    pub(super) receiver: tokio::sync::mpsc::Receiver<Result<Resolution, ProtocolError>>,
}

/// Result of sending one native permission decision back to `OpenCode`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Resolution {
    /// `OpenCode` may continue the current turn.
    Resolved,
    /// The current turn must settle as cancelled after a denial.
    Declined,
}

struct Task {
    handle: AbortOnDropHandle<()>,
    approval: Option<ApprovalRequest>,
    waiting: Arc<AtomicBool>,
}

impl Pending {
    pub(super) fn new() -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(16);
        Self {
            tasks: HashMap::new(),
            sender,
            receiver,
        }
    }

    /// Native idle state cannot settle a turn before in-flight decisions are observed.
    pub(super) fn can_settle(&self) -> bool {
        self.tasks.values().all(|task| task.handle.is_finished()) && self.receiver.is_empty()
    }

    pub(super) async fn reconcile(
        &mut self,
        api: &Api,
        request: &Invocation,
        session: &str,
    ) -> Result<(), ProtocolError> {
        let path = match api.version {
            Version::V1 => "/permission".to_owned(),
            Version::V2 => api.path(session, "/permission"),
        };
        let response = api.json(Method::GET, &path, None).await?;
        let pending = api.data(&response).as_array().ok_or_else(|| {
            failure(
                Fault::ProviderFailed,
                "invalid OpenCode pending permissions",
            )
        })?;
        for item in pending {
            if item.get("sessionID").and_then(Value::as_str) == Some(session) {
                self.observe(api, request, session, item)?;
            }
        }
        let ids = pending
            .iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .collect::<std::collections::HashSet<_>>();
        let withdrawn = self
            .tasks
            .iter()
            .filter(|(id, task)| {
                !ids.contains(id.as_str())
                    && (task.waiting.load(Ordering::Acquire) || task.handle.is_finished())
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in withdrawn {
            if let Some(task) = self.tasks.remove(&id) {
                task.handle.abort();
                if let Some(approval) = task.approval {
                    request.approvals.expire(&approval).await?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn observe(
        &mut self,
        api: &Api,
        request: &Invocation,
        session: &str,
        data: &Value,
    ) -> Result<(), ProtocolError> {
        let id = required_string(data, "id")?;
        if !valid_id(id) {
            return Err(failure(
                Fault::ProviderFailed,
                "invalid OpenCode permission identity",
            ));
        }
        if self.tasks.contains_key(id) {
            return Ok(());
        }
        if self.tasks.len() >= 16 {
            return Err(failure(
                Fault::RunLimitExceeded,
                "too many pending OpenCode approvals",
            ));
        }
        let approval = normalize(api.version, request, session, data);
        let retained_approval = approval.clone();
        let api = api.clone();
        let approvals = request.approvals.clone();
        let cancellation = request.cancellation.clone();
        let id = id.to_owned();
        let session = session.to_owned();
        let sender = self.sender.clone();
        let waiting = Arc::new(AtomicBool::new(true));
        let task_waiting = waiting.clone();
        let task = tokio::spawn(async move {
            let result = async {
                let decision = if let Some(approval) = approval {
                    tokio::select! {
                        result=approvals.decide(approval.clone())=>result?,
                        ()=cancellation.cancelled()=>{
                            approvals.expire(&approval).await?;
                            Decision::Cancelled
                        }
                    }
                } else {
                    Decision::Denied
                };
                task_waiting.store(false, Ordering::Release);
                let reply = match decision {
                    Decision::Approved => "once",
                    Decision::ApprovedAlways => "always",
                    Decision::Denied | Decision::Cancelled => "reject",
                };
                let (path, body) = match api.version {
                    Version::V1 => (format!("/permission/{id}/reply"), json!({"reply":reply})),
                    Version::V2 => (
                        api.path(&session, &format!("/permission/{id}/reply")),
                        json!({"decision":reply}),
                    ),
                };
                api.json(Method::POST, &path, Some(&body)).await?;
                approvals.resolved(&id).await?;
                Ok(match decision {
                    Decision::Approved | Decision::ApprovedAlways => Resolution::Resolved,
                    Decision::Denied | Decision::Cancelled => Resolution::Declined,
                })
            }
            .await;
            let _ = sender.send(result).await;
        });
        self.tasks.insert(
            required_string(data, "id")?.to_owned(),
            Task {
                handle: AbortOnDropHandle::new(task),
                approval: retained_approval,
                waiting,
            },
        );
        Ok(())
    }
}

fn normalize(
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

#[cfg(test)]
mod tests;
