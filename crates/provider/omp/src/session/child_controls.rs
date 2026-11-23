//! Correlated child controls through the owning parent's single native writer.

use std::collections::{BTreeMap, VecDeque};

use provider_core::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use provider_core::ports::controls::{
    ChildControlFailure, ChildControlOutcome, ChildControlReceipt, ChildGuideOutcome,
    ChildStopOutcome,
};
use serde_json::{Value, json};
use tokio::{sync::oneshot, time::Instant};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::super::{
    history::children::valid_job,
    transport::{OptionalWrite, Transport},
};

const MAX_PENDING: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Guide,
    Stop,
}

impl Kind {
    fn outcome(self, disposition: Disposition) -> ChildControlOutcome {
        match self {
            Self::Guide => ChildControlOutcome::Guide(match disposition {
                Disposition::Delivered => Ok(ChildGuideOutcome::Accepted),
                Disposition::NotRunning => Ok(ChildGuideOutcome::NotRunning),
                Disposition::Failed(error) => Err(error),
            }),
            Self::Stop => ChildControlOutcome::Stop(match disposition {
                Disposition::Delivered => Ok(ChildStopOutcome::Cancelled),
                Disposition::NotRunning => Ok(ChildStopOutcome::NotRunning),
                Disposition::Failed(error) => Err(error),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    Delivered,
    NotRunning,
    Failed(ChildControlFailure),
}

#[derive(Debug)]
struct Pending {
    token: String,
    kind: Kind,
    job: String,
    message: Option<String>,
    membership: bool,
    deadline: Instant,
    cancellation: CancellationToken,
    written: Option<oneshot::Receiver<OptionalWrite>>,
}

#[derive(Debug, Default)]
pub(super) struct Controls {
    pending: BTreeMap<String, Pending>,
}

impl Controls {
    pub(super) fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub(super) fn begin(
        &mut self,
        transport: &mut Transport,
        state: &Value,
        child: &str,
        message: Option<&str>,
    ) -> Result<ChildControlReceipt, AgentSessionError> {
        if self.pending.len() >= MAX_PENDING {
            return Err(AgentSessionError::Busy);
        }
        let parent = state["sessionId"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?;
        let job = child
            .strip_prefix(parent)
            .and_then(|suffix| suffix.strip_prefix('/'))
            .filter(|job| valid_job(job))
            .ok_or(AgentSessionError::Rejected)?;
        let cancellation = CancellationToken::new();
        let (id, written) =
            transport.queue_optional("get_subagents", json!({}), cancellation.clone())?;
        let token = Uuid::new_v4().to_string();
        self.pending.insert(
            id,
            Pending {
                token: token.clone(),
                kind: if message.is_some() {
                    Kind::Guide
                } else {
                    Kind::Stop
                },
                job: job.to_owned(),
                message: message.map(str::to_owned),
                membership: true,
                deadline: Instant::now() + transport.timeouts().request,
                cancellation,
                written: Some(written),
            },
        );
        Ok(ChildControlReceipt { token })
    }

    pub(super) fn response(
        &mut self,
        transport: &mut Transport,
        frame: &Value,
        events: &mut VecDeque<AgentTurnEvent>,
    ) -> bool {
        let Some(id) = frame["id"].as_str() else {
            return false;
        };
        let Some(mut pending) = self.pending.remove(id) else {
            return false;
        };
        let expected = if pending.membership {
            "get_subagents"
        } else if pending.kind == Kind::Guide {
            "steer_subagent"
        } else {
            "cancel_subagent"
        };
        if pending.deadline <= Instant::now() || frame["command"] != expected {
            finish(
                pending,
                Disposition::Failed(ChildControlFailure::Uncertain),
                events,
            );
        } else if pending.membership {
            match membership(frame, &pending.job) {
                Ok(true) => {
                    pending.membership = false;
                    let (command, fields) = match &pending.message {
                        Some(message) => (
                            "steer_subagent",
                            json!({"subagentId":pending.job,"message":message}),
                        ),
                        None => ("cancel_subagent", json!({"subagentId":pending.job})),
                    };
                    match transport.queue_optional(command, fields, pending.cancellation.clone()) {
                        Ok((id, written)) => {
                            pending.written = Some(written);
                            self.pending.insert(id, pending);
                        }
                        Err(_) => finish(
                            pending,
                            Disposition::Failed(ChildControlFailure::Uncertain),
                            events,
                        ),
                    }
                }
                Ok(false) => finish(pending, Disposition::NotRunning, events),
                Err(error) => finish(pending, Disposition::Failed(error), events),
            }
        } else {
            let outcome = disposition(pending.kind, frame);
            finish(pending, outcome, events);
        }
        true
    }

    pub(super) fn poll(&mut self, events: &mut VecDeque<AgentTurnEvent>) {
        let now = Instant::now();
        let mut done = Vec::new();
        for (id, pending) in &mut self.pending {
            let failed = match pending.written.as_mut().map(oneshot::Receiver::try_recv) {
                Some(Ok(OptionalWrite::Sent)) => {
                    pending.written = None;
                    false
                }
                Some(
                    Ok(OptionalWrite::Unsent | OptionalWrite::Failed)
                    | Err(oneshot::error::TryRecvError::Closed),
                ) => true,
                None | Some(Err(oneshot::error::TryRecvError::Empty)) => false,
            };
            if failed || pending.deadline <= now {
                done.push(id.clone());
            }
        }
        for id in done {
            if let Some(pending) = self.pending.remove(&id) {
                finish(
                    pending,
                    Disposition::Failed(ChildControlFailure::Uncertain),
                    events,
                );
            }
        }
    }

    pub(super) fn clear(&mut self, events: &mut VecDeque<AgentTurnEvent>) {
        for pending in std::mem::take(&mut self.pending).into_values() {
            finish(
                pending,
                Disposition::Failed(ChildControlFailure::Uncertain),
                events,
            );
        }
    }
}

fn finish(pending: Pending, disposition: Disposition, events: &mut VecDeque<AgentTurnEvent>) {
    pending.cancellation.cancel();
    events.push_back(AgentTurnEvent::ChildControl {
        token: pending.token,
        outcome: pending.kind.outcome(disposition),
    });
}

fn membership(frame: &Value, job: &str) -> Result<bool, ChildControlFailure> {
    if frame["success"] == false {
        return Err(ChildControlFailure::Rejected);
    }
    if frame["success"] != true {
        return Err(ChildControlFailure::Uncertain);
    }
    let children = frame["data"]["subagents"]
        .as_array()
        .ok_or(ChildControlFailure::Uncertain)?;
    let mut target = None;
    for child in children {
        let id = child["id"]
            .as_str()
            .filter(|id| valid_job(id))
            .ok_or(ChildControlFailure::Uncertain)?;
        if id == job && target.replace(child).is_some() {
            return Err(ChildControlFailure::Uncertain);
        }
    }
    let Some(child) = target else {
        return Ok(false);
    };
    match child["status"].as_str() {
        Some("completed" | "failed" | "aborted" | "cancelled" | "canceled") => Ok(false),
        Some("running" | "pending")
            if child["sessionFile"]
                .as_str()
                .is_some_and(|file| !file.trim().is_empty()) =>
        {
            Ok(true)
        }
        _ => Err(ChildControlFailure::Uncertain),
    }
}

fn disposition(kind: Kind, frame: &Value) -> Disposition {
    match frame["success"].as_bool() {
        Some(true) if kind == Kind::Stop => match frame["data"]["cancelled"].as_bool() {
            Some(true) => Disposition::Delivered,
            Some(false) => Disposition::NotRunning,
            None => Disposition::Failed(ChildControlFailure::Uncertain),
        },
        Some(true) => Disposition::Delivered,
        Some(false)
            if kind == Kind::Guide
                && frame["error"]
                    .as_str()
                    .is_some_and(|error| error.starts_with("Subagent not running")) =>
        {
            Disposition::NotRunning
        }
        Some(false) => Disposition::Failed(ChildControlFailure::Rejected),
        None => Disposition::Failed(ChildControlFailure::Uncertain),
    }
}

#[cfg(test)]
mod tests;
