//! Asynchronous native approvals retain exact request identity and retain native approval scope.
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::local::opencode::types::{ApprovalRequest, Decision, Invocation};
use crate::local::opencode::types::{Fault, ProtocolError};
use serde_json::Value;
use tokio_util::task::AbortOnDropHandle;

use super::{
    failure,
    protocol::http::{Api, required_string},
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
    /// Native reject was acknowledged; reconcile before deciding whether interruption is needed.
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
        let pending = api.pending_permissions(session).await?;
        for item in &pending {
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
        let approval = api.approval(request, session, data);
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
                api.reply_permission(&session, &id, decision).await?;
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

#[cfg(test)]
use super::protocol::{Version, permissions::normalize};
#[cfg(test)]
mod tests;
