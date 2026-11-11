//! Parent-owned native child controls through the managed OMP write loop.
//!
//! OMP exposes child steering and cancellation only on the parent's RPC process: `get_subagents`
//! reports the live membership of the same process that owns the write loop, and
//! `steer_subagent` / `cancel_subagent` act on those children. The manager validates the child
//! parent chain and resolves the native identity from the observed record, so this adapter
//! re-checks membership, performs at most one control write and reports the native fact.
//!
//! Commands are never awaited. The synchronous native event loop only records what to write; the
//! async hook drains those writes, and every native answer is correlated by its own frame. The
//! typed outcome is published through the ordinary event stream under the receipt token, so the
//! parent lane keeps answering queued permissions and polls while the runtime answers. No child
//! writer is created and an uncertain delivery is never resubmitted.

use std::collections::{BTreeMap, VecDeque};

use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use super::Session;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::ports::controls::{
    ChildControlFailure, ChildControlOutcome, ChildControlReceipt, ChildGuideOutcome,
    ChildStopOutcome,
};
use crate::ports::diagnostics::{Category, Diagnostic};

/// Bound on child control exchanges awaiting a native answer in one parent process.
const PENDING_CONTROLS: usize = 32;
/// Native refusal prefix reported when guidance targets a child that is no longer running.
const NOT_RUNNING: &str = "Subagent not running";

/// Native control command identity; decides which disposition an answer carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// `steer_subagent`.
    Guide,
    /// `cancel_subagent`.
    Stop,
}

impl Kind {
    /// Native command name written for this kind.
    const fn command(self) -> &'static str {
        match self {
            Self::Guide => "steer_subagent",
            Self::Stop => "cancel_subagent",
        }
    }

    /// Typed outcome for a control the native runtime never answered.
    const fn uncertain(self) -> ChildControlOutcome {
        match self {
            Self::Guide => ChildControlOutcome::Guide(Err(ChildControlFailure::Uncertain)),
            Self::Stop => ChildControlOutcome::Stop(Err(ChildControlFailure::Uncertain)),
        }
    }
}

/// What one native answer established about the requested control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// No control ran because the child was not a running member.
    NotRunning,
    /// The native runtime accepted the requested command for a running member.
    Delivered,
    /// No disposition was established.
    Failed(ChildControlFailure),
}

/// One child control exchange awaiting a native answer.
///
/// `child_id` is present while the membership re-check is still outstanding and cleared once the
/// control command itself is written, so one receipt tracks both native exchanges.
#[derive(Debug)]
pub(super) struct Pending {
    /// Receipt token reported with the eventual outcome.
    token: String,
    /// Requested control kind.
    kind: Kind,
    /// Native child to control; present only before the control command is written.
    child_id: Option<String>,
    /// Native child identity retained for the control command after the membership check.
    target: String,
    /// Guidance body for a guide request.
    message: Option<String>,
    /// Answer deadline.
    deadline: Instant,
}

impl Pending {
    /// Adapter-owned operation label for diagnostics and structured logs.
    fn operation(&self) -> &'static str {
        if self.child_id.is_some() {
            "get_subagents"
        } else {
            self.kind.command()
        }
    }
}

/// Child control writes queued by the synchronous event loop and flushed by [`Session`].
pub(super) type Queue = VecDeque<Pending>;

/// Child control exchanges already written, keyed by the native command awaiting them.
pub(super) type Written = BTreeMap<String, Pending>;

impl Session {
    /// Dispatch one steering message into a running native child of this parent process.
    ///
    /// # Errors
    /// Rejects a closed session, blank guidance, or a full pending-control budget. The native
    /// answer arrives as [`AgentTurnEvent::ChildControl`] and is never resubmitted.
    pub(super) fn guide_native_child(
        &mut self,
        child_id: &str,
        message: &str,
    ) -> Result<ChildControlReceipt, AgentSessionError> {
        if self.closed || message.trim().is_empty() {
            return Err(AgentSessionError::Rejected);
        }
        self.dispatch_child_control(Kind::Guide, child_id, Some(message.to_owned()))
    }

    /// Dispatch one hard-kill of a running native child without touching the parent turn.
    ///
    /// # Errors
    /// Rejects a closed session or a full pending-control budget; the native answer arrives as
    /// [`AgentTurnEvent::ChildControl`], and acceptance stays distinct from termination.
    pub(super) fn stop_native_child(
        &mut self,
        child_id: &str,
    ) -> Result<ChildControlReceipt, AgentSessionError> {
        if self.closed {
            return Err(AgentSessionError::Rejected);
        }
        self.dispatch_child_control(Kind::Stop, child_id, None)
    }

    /// Accept one child control and queue its membership re-check.
    fn dispatch_child_control(
        &mut self,
        kind: Kind,
        child_id: &str,
        message: Option<String>,
    ) -> Result<ChildControlReceipt, AgentSessionError> {
        if self.child_controls.len() + self.child_control_writes.len() >= PENDING_CONTROLS {
            return Err(AgentSessionError::Busy);
        }
        let token = Uuid::new_v4().to_string();
        self.child_control_writes.push_back(Pending {
            token: token.clone(),
            kind,
            child_id: Some(child_id.to_owned()),
            target: child_id.to_owned(),
            message,
            deadline: Instant::now() + self.client.deadline,
        });
        Ok(ChildControlReceipt { token })
    }

    /// Write every queued child control exchange and correlate its native answer.
    ///
    /// # Errors
    /// Returns a native transport failure only when the parent process itself is unusable; a
    /// rejected write settles that control with a typed failure instead.
    pub(super) async fn flush_child_controls(&mut self) -> Result<(), AgentSessionError> {
        while let Some(pending) = self.child_control_writes.pop_front() {
            let (kind, fields) = match &pending.child_id {
                Some(_) => ("get_subagents", json!({})),
                None => (
                    pending.kind.command(),
                    match (&pending.target, &pending.message) {
                        (target, Some(message)) => {
                            json!({"subagentId":target,"message":message})
                        }
                        (target, None) => json!({"subagentId":target}),
                    },
                ),
            };
            let written = self.transport.begin(kind, fields).await?;
            self.child_controls.insert(written, pending);
        }
        Ok(())
    }

    /// Classify one native answer to a pending child control and queue its typed outcome.
    ///
    /// Returns whether the frame belonged to a child control exchange. Every write this
    /// classification needs is queued for the next async flush, so it cannot fail here.
    pub(super) fn child_control_response(&mut self, frame: &Value) -> bool {
        let Some(id) = frame["id"].as_str() else {
            return false;
        };
        let Some(pending) = self.child_controls.remove(id) else {
            return false;
        };
        if pending.child_id.is_some() {
            self.membership_answer(pending, frame);
        } else {
            let disposition = Self::command_disposition(pending.kind, frame);
            if disposition == Disposition::Failed(ChildControlFailure::Rejected) {
                self.transport.diagnostics.observe_native(
                    &frame["error"],
                    "rpc",
                    pending.kind.command(),
                );
            }
            self.finish_control(pending, disposition);
        }
        true
    }

    /// Advance a membership re-check into the control command, or report the not-running fact.
    fn membership_answer(&mut self, mut pending: Pending, frame: &Value) {
        if frame["success"] != true {
            // The native runtime could not report membership, so no control was written.
            self.transport
                .diagnostics
                .observe_native(&frame["error"], "rpc", "get_subagents");
            self.finish_control(pending, Disposition::Failed(ChildControlFailure::Rejected));
            return;
        }
        let child_id = pending.child_id.take().unwrap_or_default();
        if !Self::member_running(&frame["data"]["subagents"], &child_id) {
            // Nothing was written, so an absent running child is a typed fact, not a failure.
            self.finish_control(pending, Disposition::NotRunning);
            return;
        }
        // The control command is written by the next async flush, never from this loop.
        self.child_control_writes.push_back(pending);
    }

    /// Whether the native membership lists `child_id` with a live session file.
    fn member_running(subagents: &Value, child_id: &str) -> bool {
        subagents.as_array().is_some_and(|children| {
            children.iter().any(|child| {
                child["id"].as_str() == Some(child_id)
                    && matches!(child["status"].as_str(), Some("running" | "pending"))
                    && child["sessionFile"]
                        .as_str()
                        .is_some_and(|file| !file.is_empty())
            })
        })
    }

    /// Classify one control answer into the disposition it establishes.
    fn command_disposition(kind: Kind, frame: &Value) -> Disposition {
        match frame["success"].as_bool() {
            // A cancel that reports nothing cancelled is a native fact, never a success.
            Some(true) if kind == Kind::Stop && frame["data"]["cancelled"] != true => {
                Disposition::NotRunning
            }
            Some(true) => Disposition::Delivered,
            Some(false)
                if kind == Kind::Guide
                    && frame["error"]
                        .as_str()
                        .is_some_and(|error| error.starts_with(NOT_RUNNING)) =>
            {
                Disposition::NotRunning
            }
            Some(false) => Disposition::Failed(ChildControlFailure::Rejected),
            None => Disposition::Failed(ChildControlFailure::Uncertain),
        }
    }

    /// Queue the typed outcome for the waiting lane reply.
    fn finish_control(&mut self, pending: Pending, disposition: Disposition) {
        let outcome = match pending.kind {
            Kind::Guide => ChildControlOutcome::Guide(match disposition {
                Disposition::Delivered => Ok(ChildGuideOutcome::Accepted),
                Disposition::NotRunning => Ok(ChildGuideOutcome::NotRunning),
                Disposition::Failed(failure) => Err(failure),
            }),
            Kind::Stop => ChildControlOutcome::Stop(match disposition {
                Disposition::Delivered => Ok(ChildStopOutcome::Cancelled),
                Disposition::NotRunning => Ok(ChildStopOutcome::NotRunning),
                Disposition::Failed(failure) => Err(failure),
            }),
        };
        self.stream.events.push_back(AgentTurnEvent::ChildControl {
            token: pending.token,
            outcome,
        });
    }

    /// Publish `Uncertain` for controls whose native answer never arrived before the deadline.
    pub(super) fn expire_child_controls(&mut self) {
        if self.child_controls.is_empty() && self.child_control_writes.is_empty() {
            return;
        }
        let now = Instant::now();
        let expired = self
            .child_controls
            .iter()
            .filter(|(_, pending)| pending.deadline <= now)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            let Some(pending) = self.child_controls.remove(&id) else {
                continue;
            };
            self.transport.diagnostics.observe_failure(
                Diagnostic::new(Category::Timeout, "child_control"),
                "OMP did not answer a child control command.",
                pending.operation(),
            );
            let outcome = pending.kind.uncertain();
            self.stream.events.push_back(AgentTurnEvent::ChildControl {
                token: pending.token,
                outcome,
            });
        }
        // A queued write whose deadline passed never reached the native runtime.
        while self
            .child_control_writes
            .front()
            .is_some_and(|pending| pending.deadline <= now)
        {
            let Some(pending) = self.child_control_writes.pop_front() else {
                break;
            };
            let outcome = pending.kind.uncertain();
            self.stream.events.push_back(AgentTurnEvent::ChildControl {
                token: pending.token,
                outcome,
            });
        }
    }

    /// Drop outstanding controls on close; waiting replies observe an uncertain outcome.
    pub(super) fn clear_child_controls(&mut self) -> Vec<AgentTurnEvent> {
        let pending = std::mem::take(&mut self.child_controls)
            .into_values()
            .chain(std::mem::take(&mut self.child_control_writes));
        pending
            .map(|pending| {
                let outcome = pending.kind.uncertain();
                AgentTurnEvent::ChildControl {
                    token: pending.token,
                    outcome,
                }
            })
            .collect()
    }
}
