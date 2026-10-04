//! The run coordinator: answers `run.dispatch`, `run.query` and `run.cancel` from the local run
//! table, routes session frames to their session task, and starts sessions.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::json;
use tokio::sync::mpsc;

use crate::config::is_loopback;
use crate::event::{Body, Event, Outcome, RejectCode};
use crate::hello::{self, Offer};
use crate::outbox::{Outbox, Outgoing};
use crate::ports::Host;
use crate::session::{self, Command, Context, Notice, Start, new_epoch, now_millis};
use crate::settings;
use crate::store::{InputRecord, RunRecord, StatusUpdate, Store};
use crate::translate::uuid_of;
use crate::wire::{
    Dispatch, Execution, Inbound, Person, REASON_DETAIL_BYTES, ReasonCode, RunState,
    SESSION_CONTRACT, Status, is_run_id, truncate,
};

/// How agents may write to Bonsai on this connection (adapter §5.5, protocol §4.2).
///
/// Sessions always load only their own MCP servers: the owner's Bonsai plugin or connector
/// usually holds a grant for every space they have, and members of the mounted space must not
/// reach the others through it. A remote Bonsai therefore gets no Bonsai tools at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WritePolicy {
    /// The Bonsai is on this machine: its MCP server can be injected into strict sessions.
    pub(crate) loopback: bool,
}

impl WritePolicy {
    /// Inject the dispatch's MCP URL as the only MCP server of a strict session.
    pub(crate) fn inject(self, provider: &str) -> bool {
        self.loopback && matches!(provider, "claude" | "codex")
    }

    /// Whether agents can write this Bonsai, and only it.
    pub(crate) fn bonsai_write(self, provider: &str) -> bool {
        self.inject(provider)
    }
}

/// Inputs to the coordinator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Input {
    /// A frame from the Hub.
    Frame(Inbound),
    /// A known frame type whose fields were unreadable.
    Malformed {
        /// Frame type.
        kind: String,
        /// Run named in it, if any.
        run_id: Option<String>,
    },
    /// `runtime.welcome` arrived.
    Connected(Person),
    /// The runtime was revoked (`4401` or handshake `401`).
    Revoked,
    /// Bonsai closed with `4400` right after this `session.events` frame.
    Rejected {
        /// Run of the frame.
        run_id: String,
        /// Its epoch.
        epoch: String,
        /// First and last sequence numbers it carried.
        from: u64,
        /// Last sequence number.
        to: u64,
    },
    /// The server is stopping.
    Shutdown,
}

/// The coordinator task's state.
pub(crate) struct Coordinator {
    host: Host,
    store: Store,
    outbox: Outbox,
    offer: Arc<RwLock<Offer>>,
    owner: Option<Person>,
    sessions: HashMap<String, mpsc::UnboundedSender<Command>>,
    /// Commands for sessions that stopped taking them but have not ended yet; answered once
    /// they end, so a run's log only ever has one writer.
    closing: HashMap<String, Vec<Command>>,
    notices: mpsc::UnboundedSender<Notice>,
    policy: WritePolicy,
    polls: Arc<std::sync::Mutex<crate::outbox::Bucket>>,
}

impl Coordinator {
    /// Create a coordinator; `notices` is where session tasks report their end.
    pub(crate) fn new(
        host: Host,
        store: Store,
        outbox: Outbox,
        offer: Arc<RwLock<Offer>>,
        notices: mpsc::UnboundedSender<Notice>,
        policy: WritePolicy,
    ) -> Self {
        let owner = store.owner().ok().flatten();
        Self {
            host,
            store,
            outbox,
            offer,
            owner,
            sessions: HashMap::new(),
            closing: HashMap::new(),
            notices,
            policy,
            polls: Arc::new(std::sync::Mutex::new(crate::outbox::Bucket::new(2.0, 2.0))),
        }
    }

    /// Serve inputs and session notices until shutdown.
    pub(crate) async fn run(
        mut self,
        mut inputs: mpsc::UnboundedReceiver<Input>,
        mut notices: mpsc::UnboundedReceiver<Notice>,
    ) {
        loop {
            tokio::select! {
                input = inputs.recv() => match input {
                    Some(Input::Shutdown) | None => {
                        self.broadcast(&Command::Shutdown);
                        return;
                    }
                    Some(input) => self.handle(input),
                },
                Some(Notice::Ended(run_id)) = notices.recv() => self.ended(&run_id),
            }
        }
    }

    /// Handle one input synchronously; slow work happens in session tasks.
    pub(crate) fn handle(&mut self, input: Input) {
        match input {
            Input::Frame(frame) => self.frame(frame),
            Input::Malformed { kind, run_id } => self.malformed(&kind, run_id),
            Input::Connected(owner) => {
                if let Err(error) = self.store.set_owner(&owner) {
                    tracing::warn!(?error, "recording the machine owner failed");
                }
                self.owner = Some(owner.clone());
                self.broadcast(&Command::Owner(owner));
            }
            Input::Revoked => self.broadcast(&Command::Revoke),
            Input::Rejected {
                run_id,
                epoch,
                from,
                to,
            } => {
                let command = Command::Quarantine {
                    epoch: epoch.clone(),
                    from,
                    to,
                };
                if self.forward(&run_id, command).is_err() {
                    session::quarantine(&self.store, &run_id, &epoch, from, to);
                }
            }
            Input::Shutdown => self.broadcast(&Command::Shutdown),
        }
    }

    fn frame(&mut self, frame: Inbound) {
        match frame {
            Inbound::Dispatch(dispatch) => self.dispatch(&dispatch),
            // The Hub does not count the status that answers its own question (protocol §2).
            Inbound::Query { run_id } => {
                self.outbox.send(Outgoing::Credit {
                    run_id: run_id.clone(),
                });
                self.query(&run_id);
            }
            Inbound::Cancel { run_id } => {
                self.outbox.send(Outgoing::Credit {
                    run_id: run_id.clone(),
                });
                self.cancel(&run_id);
            }
            Inbound::Subscribe { run_id, sub, after } => {
                self.outbox.send(Outgoing::Answer { run_id, sub, after });
            }
            Inbound::Send {
                run_id,
                by,
                input_id,
                text,
            } => self.send(run_id, by, input_id, text),
            Inbound::Interrupt { run_id } => {
                let _ = self.forward(&run_id, Command::Interrupt);
            }
            Inbound::Answer(answer) => {
                let run_id = answer.run_id.clone();
                let reference = answer.ask_id.clone();
                if self
                    .forward(&run_id, Command::Answer(Box::new(answer)))
                    .is_err()
                    && !self.known(&run_id)
                {
                    self.outbox
                        .send(Outgoing::Unavailable { run_id, reference });
                }
            }
            Inbound::Welcome(_) | Inbound::Unknown => {}
        }
    }

    fn malformed(&mut self, kind: &str, run_id: Option<String>) {
        tracing::warn!(kind, "ignored an unreadable frame from Bonsai");
        if kind != "run.dispatch" {
            return;
        }
        let Some(run_id) = run_id.filter(|run_id| is_run_id(run_id)) else {
            return;
        };
        if self.known(&run_id) {
            self.query(&run_id);
        } else {
            self.status_frame(
                &run_id,
                RunState::Failed,
                None,
                Some(ReasonCode::Rejected),
                Some("派发帧读不懂"),
            );
        }
    }

    /// Hand a command to a live session; gives it back when there is none.
    ///
    /// A session that stopped taking commands may still be writing its log, so a command it
    /// refuses waits until the session ends (`ended`) and is answered from the store then.
    fn forward(&mut self, run_id: &str, command: Command) -> Result<(), Command> {
        if let Some(waiting) = self.closing.get_mut(run_id) {
            waiting.push(command);
            return Ok(());
        }
        let Some(session) = self.sessions.get(run_id) else {
            return Err(command);
        };
        match session.send(command) {
            Ok(()) => Ok(()),
            Err(returned) => {
                self.sessions.remove(run_id);
                self.closing.insert(run_id.to_owned(), vec![returned.0]);
                Ok(())
            }
        }
    }

    /// A session task finished: answer what reached it too late.
    pub(crate) fn ended(&mut self, run_id: &str) {
        self.sessions.remove(run_id);
        for command in self.closing.remove(run_id).unwrap_or_default() {
            match command {
                Command::Send { by, input_id, text } => {
                    self.reject_late_input(run_id.to_owned(), &by, input_id, text);
                }
                Command::Cancel => self.cancel(run_id),
                Command::Quarantine { epoch, from, to } => {
                    session::quarantine(&self.store, run_id, &epoch, from, to);
                }
                Command::Answer(_)
                | Command::Interrupt
                | Command::Owner(_)
                | Command::Revoke
                | Command::Shutdown => {}
            }
        }
    }

    fn known(&self, run_id: &str) -> bool {
        self.store.run(run_id).ok().flatten().is_some()
    }

    /// Protocol §4.2 and adapter §4.1: tombstone, duplicate, record first, validate, claim, start.
    fn dispatch(&mut self, dispatch: &Dispatch) {
        let run_id = dispatch.run_id.clone();
        if self.store.is_buried(&run_id).unwrap_or(false) {
            self.status_frame(&run_id, RunState::Cancelled, None, None, None);
            return;
        }
        if self.known(&run_id) {
            self.query(&run_id);
            return;
        }
        let at = now_millis();
        match self.store.insert_run(dispatch, &new_epoch(), at) {
            Ok(true) => {}
            Ok(false) => {
                self.query(&run_id);
                return;
            }
            Err(error) => {
                tracing::error!(?error, "recording a dispatch failed");
                return;
            }
        }
        let resolved = match self.resolve(dispatch) {
            Ok(resolved) => resolved,
            Err((code, detail)) => {
                self.finish(&run_id, RunState::Failed, None, Some(code), Some(&detail));
                return;
            }
        };
        // Every status from `claimed` on must carry the execution (protocol §4.3); one the
        // store cannot hold would be answered without it on every later `run.query`.
        if let Err(error) = self.store.set_execution(&run_id, &resolved.execution) {
            tracing::error!(?error, "recording the execution failed");
            self.finish(
                &run_id,
                RunState::Failed,
                None,
                Some(ReasonCode::Rejected),
                Some("执行端记不下这次派发"),
            );
            return;
        }
        let _ = self.store.set_mode(&run_id, Some(&resolved.applied.mode));
        self.status_frame(
            &run_id,
            RunState::Claimed,
            Some(resolved.execution.clone()),
            None,
            None,
        );
        let Ok(Some(run)) = self.store.run(&run_id) else {
            return;
        };
        self.start(run, resolved);
    }

    /// Validate a dispatch against what this runtime announced and resolve its settings.
    fn resolve(&self, dispatch: &Dispatch) -> Result<Resolved, (ReasonCode, String)> {
        if dispatch.session != SESSION_CONTRACT {
            return Err((
                ReasonCode::Rejected,
                format!("不支持的会话契约:{}", dispatch.session),
            ));
        }
        let offer = self
            .offer
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let project = offer.project(&dispatch.project.id).ok_or_else(|| {
            (
                ReasonCode::ProjectUnavailable,
                "这台机器上没有这个项目".to_owned(),
            )
        })?;
        let provider = match &dispatch.provider {
            Some(id) => offer.provider(id),
            None => offer.default_provider(),
        }
        .ok_or_else(|| {
            (
                ReasonCode::ProviderUnavailable,
                "这个 provider 在这台机器上不可用".to_owned(),
            )
        })?;
        if let Some(model) = &dispatch.model
            && !provider.models.iter().any(|known| &known.id == model)
        {
            return Err((
                ReasonCode::ProviderUnavailable,
                format!("{} 没有这个模型:{model}", provider.id),
            ));
        }
        let applied = settings::apply(
            &provider.id,
            &provider.settings,
            &dispatch.settings,
            provider.model_traits(dispatch.model.as_deref()),
        )
        .map_err(|reason| (ReasonCode::Rejected, reason))?;
        let inject = (self.policy.inject(&provider.id)
            && url::Url::parse(&dispatch.bonsai.mcp_url).is_ok_and(|url| is_loopback(&url)))
        .then(|| dispatch.bonsai.mcp_url.clone());
        // Writes are confined to the dispatch's MCP URL only when it is injected (strict).
        let bonsai_write = inject.is_some();
        Ok(Resolved {
            execution: Execution {
                provider: provider.id.clone(),
                model: dispatch.model.clone(),
                approvals: applied.approvals,
                bonsai_write,
            },
            project: project.host_id.clone(),
            applied,
            inject,
        })
    }

    /// Re-attach every run left open by the previous process (adapter §5.3). Runs before the
    /// connection starts, so the Hub's reconciliation finds sessions where they exist.
    pub(crate) async fn recover(&mut self) {
        let Ok(runs) = self.store.open_runs() else {
            tracing::error!("reading open runs failed; nothing recovered");
            return;
        };
        for run in runs {
            let agent_id = uuid_of(run.run_id.trim_start_matches("r_"));
            let snapshot = hello::execute_retrying(
                self.host.executor.as_ref(),
                "agent.get.request",
                json!({"agentId": agent_id}),
            )
            .await;
            // Attach only to an Agent that carries this run's label: the derived ID alone could
            // name one of the owner's own sessions. Without a readable snapshot that cannot be
            // checked, so the run ends there too (and the Agent is left untouched).
            let snapshot = match snapshot {
                Ok(value)
                    if !value["agent"].is_null() && labelled_for(&value["agent"], &run.run_id) =>
                {
                    value
                }
                other => {
                    let detail = match other {
                        Err(code) if code != model::ErrorCode::AgentNotFound => {
                            "执行端重启之后读不到这个会话的状态"
                        }
                        _ => "执行端重启之后接不回这个会话",
                    };
                    let state = if run.cancel_requested {
                        RunState::Cancelled
                    } else {
                        RunState::Failed
                    };
                    let reason = (state == RunState::Failed).then_some(ReasonCode::SessionLost);
                    self.close_log(&run, "error");
                    self.finish(
                        &run.run_id,
                        state,
                        run.execution.clone(),
                        reason,
                        reason.map(|_| detail),
                    );
                    continue;
                }
            };
            if run.agent_id.is_none() {
                let _ = self.store.set_agent(&run.run_id, &agent_id);
            }
            if run.status == RunState::Claimed {
                // The Agent was created before the restart: the session is running.
                let update = StatusUpdate {
                    status: RunState::Running,
                    reason_code: None,
                    reason_detail: None,
                    final_text: None,
                    at: now_millis(),
                };
                let _ = self.store.advance(&run.run_id, &update);
            }
            if snapshot["agent"]["archivedAt"].is_string() {
                self.close_log(&run, "archived");
                self.finish(
                    &run.run_id,
                    RunState::Completed,
                    run.execution.clone(),
                    None,
                    None,
                );
                continue;
            }
            let Some(execution) = run.execution.clone() else {
                self.finish(
                    &run.run_id,
                    RunState::Failed,
                    None,
                    Some(ReasonCode::SessionLost),
                    Some("执行端重启之后接不回这个会话"),
                );
                continue;
            };
            let mode = run
                .mode
                .clone()
                .unwrap_or_else(|| settings::default_mode(&execution.provider).to_owned());
            let run_id = run.run_id.clone();
            let start = Start {
                run,
                execution,
                project: String::new(),
                applied: settings::Applied {
                    mode,
                    ..settings::Applied::default()
                },
                owner: self.owner.clone(),
                inject: None,
            };
            let sender = session::recover::spawn(self.context(), start, snapshot);
            self.sessions.insert(run_id, sender);
        }
    }

    /// Close a run's log when no session will: settle its requests, reject its queued
    /// inputs, then `closed` (protocol §4.4, §6).
    fn close_log(&self, run: &RunRecord, reason: &str) {
        let mut bodies = Vec::new();
        for mut ask in self.store.asks(&run.run_id).unwrap_or_default() {
            if ask.state == "resolved" {
                continue;
            }
            // An answer already on its way keeps its effect, as when a session recovers.
            let by = ask
                .resolving_by
                .as_deref()
                .filter(|_| ask.state == "resolving")
                .and_then(|by| serde_json::from_str::<Person>(by).ok());
            let outcome = match (&by, ask.resolving_effect.as_deref()) {
                (Some(_), Some("allow")) => Outcome::Allow,
                (Some(_), _) => Outcome::Deny,
                (None, _) => Outcome::Withdrawn,
            };
            bodies.push(Body::AskResolved {
                id: ask.ask_id.clone(),
                outcome,
                by: by.as_ref().map(|person| person.id.clone()),
                login: by.and_then(|person| person.login),
            });
            "resolved".clone_into(&mut ask.state);
            let _ = self.store.put_ask(&run.run_id, &ask);
        }
        for input in self.store.inputs(&run.run_id).unwrap_or_default() {
            if input.state == "queued" {
                let _ = self
                    .store
                    .set_input_state(&run.run_id, &input.input_id, "rejected");
                bodies.push(Body::InputRejected {
                    id: input.input_id,
                    code: RejectCode::Closed,
                    reason: None,
                });
            }
        }
        bodies.push(Body::Closed {
            reason: Some(reason.to_owned()),
        });
        let at = now_millis();
        let events: Vec<String> = bodies
            .into_iter()
            .zip(run.next_seq..)
            .filter_map(|(body, seq)| Event { seq, at, body }.to_json().ok())
            .collect();
        let _ = self
            .store
            .append(&run.run_id, &run.epoch, run.next_seq, &events, None);
    }

    fn context(&self) -> Context {
        Context {
            host: self.host.clone(),
            store: self.store.clone(),
            outbox: self.outbox.clone(),
            notices: self.notices.clone(),
            polls: Arc::clone(&self.polls),
        }
    }

    fn start(&mut self, run: RunRecord, resolved: Resolved) {
        let run_id = run.run_id.clone();
        let sender = session::spawn(
            self.context(),
            Start {
                run,
                execution: resolved.execution,
                project: resolved.project,
                applied: resolved.applied,
                owner: self.owner.clone(),
                inject: resolved.inject,
            },
        );
        self.sessions.insert(run_id, sender);
    }

    fn query(&self, run_id: &str) {
        match self.store.run(run_id) {
            Ok(Some(run)) => self.send_record(&run),
            Ok(None) if self.store.is_buried(run_id).unwrap_or(false) => {
                self.status_frame(run_id, RunState::Cancelled, None, None, None);
            }
            Ok(None) => {
                self.status_frame(
                    run_id,
                    RunState::Failed,
                    None,
                    Some(ReasonCode::RunUnknown),
                    None,
                );
            }
            Err(error) => tracing::error!(?error, "reading a run failed"),
        }
    }

    /// Protocol §4.4, the parts the coordinator answers itself.
    fn cancel(&mut self, run_id: &str) {
        let run = match self.store.run(run_id) {
            Ok(run) => run,
            Err(error) => {
                tracing::error!(?error, "reading a run failed");
                return;
            }
        };
        let Some(run) = run else {
            let _ = self.store.bury(run_id, now_millis());
            self.status_frame(run_id, RunState::Cancelled, None, None, None);
            return;
        };
        if run.status.is_terminal() {
            self.send_record(&run);
            return;
        }
        let _ = self.store.request_cancel(run_id);
        if self.forward(run_id, Command::Cancel).is_ok() {
            return;
        }
        // No live session: nothing is running for this run any more.
        let state = if run.agent_id.is_some() && run.status == RunState::Running {
            RunState::Completed
        } else {
            RunState::Cancelled
        };
        self.finish(run_id, state, run.execution.clone(), None, None);
    }

    fn send(&mut self, run_id: String, by: Person, input_id: String, text: String) {
        let command = Command::Send { by, input_id, text };
        let Err(Command::Send { by, input_id, text }) = self.forward(&run_id, command) else {
            return;
        };
        self.reject_late_input(run_id, &by, input_id, text);
    }

    /// Answer input for a run with no session: `unavailable` for an unknown run, otherwise one
    /// `input_rejected{closed}` in the run's own log.
    fn reject_late_input(&mut self, run_id: String, by: &Person, input_id: String, text: String) {
        let Ok(Some(run)) = self.store.run(&run_id) else {
            self.outbox.send(Outgoing::Unavailable {
                run_id,
                reference: input_id,
            });
            return;
        };
        // The session is over: reject the input once, in the run's own log.
        let record = InputRecord {
            message_id: uuid_of(&input_id),
            input_id: input_id.clone(),
            text,
            state: "rejected".to_owned(),
            by: Some(by.id.clone()),
            login: by.login.clone(),
        };
        if !self.store.insert_input(&run_id, &record).unwrap_or(false) {
            return;
        }
        let event = Event {
            seq: run.next_seq,
            at: now_millis(),
            body: Body::InputRejected {
                id: input_id,
                code: RejectCode::Closed,
                reason: None,
            },
        };
        let Ok(json) = event.to_json() else {
            return;
        };
        if self
            .store
            .append(&run_id, &run.epoch, run.next_seq, &[json], None)
            .is_ok()
        {
            self.outbox.send(Outgoing::Live { run_id });
        }
    }

    fn finish(
        &self,
        run_id: &str,
        status: RunState,
        execution: Option<Execution>,
        reason_code: Option<ReasonCode>,
        reason_detail: Option<&str>,
    ) {
        let detail = reason_detail.map(|detail| truncate(detail, REASON_DETAIL_BYTES).0.to_owned());
        let update = StatusUpdate {
            status,
            reason_code,
            reason_detail: detail,
            final_text: None,
            at: now_millis(),
        };
        if let Err(error) = self.store.advance(run_id, &update) {
            tracing::error!(?error, "recording the run state failed");
        }
        self.status_frame(run_id, status, execution, reason_code, reason_detail);
    }

    fn send_record(&self, run: &RunRecord) {
        self.outbox.send(Outgoing::Status(Status {
            kind: "run.status",
            run_id: run.run_id.clone(),
            status: run.status,
            at: run.status_at,
            execution: run.execution.clone(),
            reason_code: if run.status == RunState::Failed {
                run.reason_code
            } else {
                None
            },
            reason_detail: if run.status == RunState::Failed {
                run.reason_detail.clone()
            } else {
                None
            },
            final_text: run.final_text.clone(),
        }));
    }

    fn status_frame(
        &self,
        run_id: &str,
        status: RunState,
        execution: Option<Execution>,
        reason_code: Option<ReasonCode>,
        reason_detail: Option<&str>,
    ) {
        self.outbox.send(Outgoing::Status(Status {
            kind: "run.status",
            run_id: run_id.to_owned(),
            status,
            at: now_millis(),
            execution,
            reason_code,
            reason_detail: reason_detail
                .map(|detail| truncate(detail, REASON_DETAIL_BYTES).0.to_owned()),
            final_text: None,
        }));
    }

    fn broadcast(&self, command: &Command) {
        for session in self.sessions.values() {
            let _ = session.send(command.clone());
        }
    }
}

/// Whether an Agent snapshot carries this run's label: the derived Agent ID alone must never
/// attach one of the owner's own sessions (adapter §3.3).
fn labelled_for(agent: &serde_json::Value, run_id: &str) -> bool {
    agent["labels"]["bonsai.run"] == run_id
}

/// A dispatch that passed validation.
struct Resolved {
    execution: Execution,
    project: String,
    applied: settings::Applied,
    inject: Option<String>,
}

#[cfg(test)]
mod tests;
