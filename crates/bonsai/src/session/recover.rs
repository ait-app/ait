//! Re-attaching a session after AIT restarted (adapter §5.3).

use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{Ask, AskState, Command, Context, Messages, Notice, Queued, Session, Start};
use crate::event::{Body, Effect, Event, Outcome, TurnState};
use crate::translate::{AskSpec, uuid_of};
use crate::wire::Person;

/// What the stored log says about a session at the moment the server stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LogState {
    /// A turn had started and not ended.
    pub(crate) turn_open: bool,
    /// The last turn ended in failure.
    pub(crate) last_turn_failed: bool,
    /// The session was closed, and why.
    pub(crate) closed: Option<String>,
    /// Assistant messages, for `final_text`.
    pub(crate) messages: Messages,
    /// A sub-agent notice was already shown.
    pub(crate) subagent_noted: bool,
}

/// Fold a stored log into the state recovery needs.
pub(crate) fn fold(events: &[String]) -> LogState {
    let mut state = LogState::default();
    for json in events {
        let Ok(event) = serde_json::from_str::<Event>(json) else {
            continue;
        };
        match event.body {
            Body::Turn { state: turn, .. } => {
                state.turn_open = turn == TurnState::Started && state.closed.is_none();
                if turn != TurnState::Started {
                    state.last_turn_failed = turn == TurnState::Failed;
                }
            }
            Body::Text { mid, text } => state.messages.append(&mid, &text),
            Body::Closed { reason } => {
                state.closed = Some(reason.unwrap_or_default());
                state.turn_open = false;
            }
            Body::Notice { text, .. } if text == super::SUBAGENT_NOTICE => {
                state.subagent_noted = true;
            }
            _ => {}
        }
    }
    state
}

/// Spawn a session for a run whose Agent survived a restart; `snapshot` is its `agent.get`.
pub(crate) fn spawn(
    context: Context,
    start: Start,
    snapshot: Value,
) -> mpsc::UnboundedSender<Command> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let run_id = start.run.run_id.clone();
        let notices = context.notices.clone();
        let mut session = Session::new(context, &start);
        let mut receiver = receiver;
        session
            .run_recovered(&start, &snapshot, &mut receiver)
            .await;
        session.drain(&mut receiver).await;
        let _ = notices.send(Notice::Ended(run_id));
    });
    sender
}

impl Session {
    async fn run_recovered(
        &mut self,
        start: &Start,
        snapshot: &Value,
        commands: &mut mpsc::UnboundedReceiver<Command>,
    ) {
        let Ok(closed_reason) = self.restore(start, snapshot) else {
            // Our own log is unreadable: the session cannot be shown or continued, and what
            // arrives meanwhile is answered as for a closed session.
            self.closed = true;
            self.archive().await;
            self.report(
                crate::wire::RunState::Failed,
                Some(crate::wire::ReasonCode::SessionLost),
                Some("执行端本地的会话记录读不出来".to_owned()),
            );
            return;
        };
        if let Some(reason) = closed_reason {
            // The session had closed but the final state was never recorded.
            let cancelled = reason == "cancelled" || self.cancel_requested();
            if cancelled {
                self.report(crate::wire::RunState::Cancelled, None, None);
            } else if self.last_turn_failed {
                self.report(
                    crate::wire::RunState::Failed,
                    Some(crate::wire::ReasonCode::ProviderError),
                    Some("最后一轮失败之后没有新的输入".to_owned()),
                );
            } else {
                self.report(crate::wire::RunState::Completed, None, None);
            }
            return;
        }
        let observation = self.context.host.observer.observe(&self.agent_id).ok();
        // Let AIT reconcile the native history first; it may rotate the timeline.
        let _ = self
            .context
            .host
            .executor
            .execute(
                "agent.timeline.get.request",
                json!({"agentId": self.agent_id, "direction": "tail", "limit": 1}),
            )
            .await;
        self.backfill_with(true).await;
        self.observation = observation
            .and_then(|mut observation| observation.activate().is_ok().then_some(observation));
        if self.observation.is_none() {
            self.retry_observe();
        }
        if self.turn_open {
            self.withdraw_open_asks();
            self.push(Body::Turn {
                state: TurnState::Aborted,
                reason: Some("runtime_restarted".to_owned()),
            });
            self.turn_open = false;
        }
        self.flush();
        if self.cancel_requested() {
            // A cancel recorded before the restart: the restart already stopped the turn.
            self.finish_cancel().await;
            return;
        }
        self.deliver_next().await;
        self.serve(commands).await;
        self.flush();
    }

    /// Rebuild in-memory state from the store and the Agent snapshot.
    ///
    /// Returns why the logged session closed, if it did; `Err` when our log is unreadable.
    fn restore(&mut self, start: &Start, snapshot: &Value) -> Result<Option<String>, ()> {
        let run = &start.run;
        let events = self
            .context
            .store
            .events(&run.run_id, &run.epoch, 0, run.next_seq)
            .map_err(|_| ())?;
        let state = fold(&events);
        self.turn_open = state.turn_open;
        self.last_turn_failed = state.last_turn_failed;
        self.closed = state.closed.is_some();
        self.messages = state.messages;
        self.subagent_noted = state.subagent_noted;
        self.translator
            .sent(&uuid_of(&format!("{}:dispatch", self.run_id)));
        for input in self.context.store.inputs(&self.run_id).unwrap_or_default() {
            self.translator.sent(&input.message_id);
            if input.state == "queued" {
                self.queue.push_back(Queued {
                    input_id: input.input_id,
                    message_id: input.message_id,
                    text: input.text,
                });
            }
        }
        let pending: Vec<&str> = snapshot["agent"]["pendingPermissions"]
            .as_array()
            .map(|requests| {
                requests
                    .iter()
                    .filter_map(|request| request["id"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        for record in self.context.store.asks(&self.run_id).unwrap_or_default() {
            let Ok(spec) = serde_json::from_str::<AskSpec>(&record.spec) else {
                continue;
            };
            let still_pending = pending.contains(&spec.native_id());
            let state = match record.state.as_str() {
                "resolved" => AskState::Resolved,
                "resolving" if !still_pending => {
                    let by = record
                        .resolving_by
                        .as_deref()
                        .and_then(|by| serde_json::from_str::<Person>(by).ok());
                    let effect = if record.resolving_effect.as_deref() == Some("allow") {
                        Effect::Allow
                    } else {
                        Effect::Deny
                    };
                    self.asks.insert(
                        spec.id.clone(),
                        Ask {
                            spec: spec.clone(),
                            state: AskState::Pending { attempt: None },
                        },
                    );
                    let outcome = if effect == Effect::Allow {
                        Outcome::Allow
                    } else {
                        Outcome::Deny
                    };
                    self.settle(&spec.id, outcome, by);
                    continue;
                }
                _ if still_pending => AskState::Pending { attempt: None },
                _ => {
                    // The provider forgot the request across the restart (Claude does).
                    self.asks.insert(
                        spec.id.clone(),
                        Ask {
                            spec: spec.clone(),
                            state: AskState::Pending { attempt: None },
                        },
                    );
                    self.settle(&spec.id, Outcome::Withdrawn, None);
                    continue;
                }
            };
            self.asks.insert(spec.id.clone(), Ask { spec, state });
        }
        Ok(state.closed)
    }
}

#[cfg(test)]
mod tests;
