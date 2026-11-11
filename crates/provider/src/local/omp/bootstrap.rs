//! Publish standard startup interactions before the external runtime finishes initializing.
//!
//! The provisional session answers dialogs through the same serialized native writer that
//! initialization uses. Once the native session is ready it adopts the remaining dialogs and
//! releases the single queued first input; a cancelled input is never replayed.
//! Startup failure settles through the manager's durable error state and timeline, not a
//! fabricated native persistence handle. Closing also releases a ready-but-unadopted session;
//! cancelled initialization remains a draft and cannot be mistaken for resumable native history.

use std::borrow::Cow;
use std::collections::VecDeque;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::task::AbortOnDropHandle;

use super::{OmpClient, PROVIDER, config, interactions::Dialogs, session, transport::Writer};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

/// Startup frames retain native request identity and use the same single serialized writer.
#[derive(Debug)]
pub(super) enum Frame {
    Writer(Writer),
    Ui(Value),
}

type Ready = Result<session::Session, AgentSessionError>;

#[derive(Debug)]
struct Input {
    turn: String,
    prompt: AgentPrompt,
    config: StoredAgentConfig,
}

#[derive(Debug)]
struct Session {
    receiver: mpsc::Receiver<Frame>,
    ready: oneshot::Receiver<Ready>,
    task: Option<AbortOnDropHandle<()>>,
    inner: Option<session::Session>,
    writer: Option<Writer>,
    dialogs: Dialogs,
    events: VecDeque<AgentTurnEvent>,
    input: Option<Input>,
    turns: Option<(String, String)>,
    closed: bool,
    /// Startup/admission failure settles once through the ordinary manager drain.
    failed: bool,
    failure: Option<String>,
}

/// Keep normal initialization unchanged; return early only to let the user answer a startup
/// dialog. Invalid native initialization remains an error, and cancellation never replays input.
pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Box<dyn AgentSession>, AgentSessionError> {
    config::validate_spec(spec)?;
    let (sender, receiver) = mpsc::channel(128);
    let (finished, ready) = oneshot::channel();
    let launch = spec.clone();
    let restore = handle.cloned();
    let client = client.clone();
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        let result = session::open(&client, &launch, restore.as_ref(), Some(sender)).await;
        let _ = finished.send(result);
    }));
    let mut session = Session {
        receiver,
        ready,
        task: Some(task),
        inner: None,
        writer: None,
        dialogs: Dialogs::default(),
        events: VecDeque::new(),
        input: None,
        turns: None,
        closed: false,
        failed: false,
        failure: None,
    };
    loop {
        tokio::select! {
            ready = &mut session.ready => {
                let mut inner = ready.map_err(|_| AgentSessionError::Failed)??;
                inner.adopt_startup(std::mem::take(&mut session.dialogs))?;
                return Ok(Box::new(inner));
            }
            frame = session.receiver.recv() => {
                let Some(frame) = frame else {
                    let mut inner = session.ready.await.map_err(|_| AgentSessionError::Failed)??;
                    inner.adopt_startup(std::mem::take(&mut session.dialogs))?;
                    return Ok(Box::new(inner));
                };
                session.frame(frame)?;
                if !session.dialogs.pending().is_empty() {
                    return Ok(Box::new(session));
                }
            }
        }
    }
}

impl Session {
    fn frame(&mut self, frame: Frame) -> Result<(), AgentSessionError> {
        match frame {
            Frame::Writer(writer) => self.writer = Some(writer),
            Frame::Ui(frame) => {
                self.events.extend(self.dialogs.capture(&frame)?);
            }
        }
        Ok(())
    }

    /// Adopt the ready native session and release the single queued input through it.
    /// Startup, adoption and queued-admission failures become one terminal Failed event
    /// so the manager commits the failure, closes the writer and releases the lane.
    async fn advance(&mut self) -> Result<(), AgentSessionError> {
        if self.failed || self.closed {
            return Ok(());
        }
        while let Ok(frame) = self.receiver.try_recv() {
            if let Err(error) = self.frame(frame) {
                self.fail(error);
                return Ok(());
            }
        }
        if self.inner.is_none() {
            match self.ready.try_recv() {
                Ok(result) => {
                    let adopted = async {
                        let mut inner = result?;
                        inner.adopt_startup(std::mem::take(&mut self.dialogs))?;
                        let info = inner.runtime_info().await?;
                        Ok::<_, AgentSessionError>((inner, info))
                    }
                    .await;
                    match adopted {
                        Ok((inner, info)) => {
                            self.writer.take();
                            self.task.take();
                            self.events.push_back(AgentTurnEvent::RuntimeInfo(info));
                            self.inner = Some(inner);
                        }
                        Err(error) => {
                            self.fail(error);
                            return Ok(());
                        }
                    }
                }
                Err(oneshot::error::TryRecvError::Empty) => return Ok(()),
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.fail(AgentSessionError::Failed);
                    return Ok(());
                }
            }
        }
        if let Some(inner) = &mut self.inner
            && let Some(input) = self.input.take()
        {
            match inner.start_input(&input.prompt, &input.config).await {
                Ok(turn) => self.turns = Some((input.turn, turn)),
                Err(error) => {
                    self.fail(error);
                }
            }
        }
        Ok(())
    }

    /// Settle a startup failure exactly once: withdraw pending dialogs, drop the queued
    /// input without replaying it, and queue the terminal event for the manager drain.
    fn fail(&mut self, error: AgentSessionError) {
        if self.failed {
            return;
        }
        self.failed = true;
        self.failure = Some(match error {
            AgentSessionError::Rejected => {
                "OMP rejected the session startup or its first input.".to_owned()
            }
            AgentSessionError::Unavailable => "OMP became unavailable during startup.".to_owned(),
            _ => "OMP native initialization failed before the first input was admitted.".to_owned(),
        });
        self.input.take();
        self.turns.take();
        self.writer.take();
        if let Some(task) = &self.task {
            task.abort();
        }
        self.receiver.close();
        self.events.extend(self.dialogs.clear());
        self.events.push_back(AgentTurnEvent::Failed);
    }

    fn inner(&mut self) -> Result<&mut session::Session, AgentSessionError> {
        self.inner.as_mut().ok_or(AgentSessionError::Busy)
    }

    /// Map the native turn identity back to the logical turn published before initialization.
    fn event(&mut self, event: AgentTurnEvent) -> AgentTurnEvent {
        let Some((logical, native)) = &self.turns else {
            return event;
        };
        match event {
            AgentTurnEvent::Started(turn) if &turn == native => {
                AgentTurnEvent::Started(logical.clone())
            }
            AgentTurnEvent::Timeline(mut entry) => {
                if entry.turn_id.is_none() || entry.turn_id.as_ref() == Some(native) {
                    entry.turn_id = Some(logical.clone());
                }
                AgentTurnEvent::Timeline(entry)
            }
            AgentTurnEvent::Progress {
                observation,
                mut entry,
            } => {
                if entry.turn_id.is_none() || entry.turn_id.as_ref() == Some(native) {
                    entry.turn_id = Some(logical.clone());
                }
                AgentTurnEvent::Progress { observation, entry }
            }
            event => event,
        }
    }
}

#[async_trait::async_trait]
impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        self.inner.as_ref().and_then(AgentSession::persistence)
    }

    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        self.inner
            .as_ref()
            .and_then(|inner| inner.control_settings(config))
    }

    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        self.inner.as_ref().map_or_else(
            || config::validate(config),
            |inner| inner.validate_config_update(config),
        )
    }

    fn pending_permissions(&self) -> Vec<Value> {
        self.inner
            .as_ref()
            .map_or_else(|| self.dialogs.pending(), AgentSession::pending_permissions)
    }

    fn subagents(&self) -> Vec<crate::ports::controls::NativeSubagent> {
        self.inner
            .as_ref()
            .map_or_else(Vec::new, AgentSession::subagents)
    }

    fn failure_diagnostic(&self) -> Option<crate::ports::diagnostics::Diagnostic> {
        self.inner
            .as_ref()
            .and_then(AgentSession::failure_diagnostic)
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        if let Some(message) = &self.failure {
            return Some(Cow::Borrowed(message.as_str()));
        }
        self.inner.as_ref().and_then(AgentSession::failure_message)
    }

    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        if let Some(inner) = &mut self.inner {
            return inner.runtime_info().await;
        }
        // The native identity is unknown until initialization finishes; publish nothing
        // rather than a guessed session that could misroute later observations.
        Ok(StoredAgentRuntimeInfo {
            provider: PROVIDER.to_owned(),
            session_id: None,
            model: None,
            thinking_option_id: None,
            mode_id: None,
            extra: None,
        })
    }

    async fn service_realtime(&mut self) -> Result<(), AgentSessionError> {
        self.advance().await?;
        if let Some(inner) = &mut self.inner {
            inner.service_realtime().await?;
        }
        Ok(())
    }

    async fn start_turn(
        &mut self,
        text: &str,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        self.start_input(&AgentPrompt::text(text), config).await
    }

    async fn start_input(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        if self.closed || self.failed {
            return Err(AgentSessionError::Unavailable);
        }
        if let Some(inner) = &mut self.inner {
            return inner.start_input(prompt, config).await;
        }
        prompt.validate()?;
        config::validate(config)?;
        if self.input.is_some() || self.turns.is_some() {
            return Err(AgentSessionError::Busy);
        }
        let turn = uuid::Uuid::new_v4().to_string();
        self.input = Some(Input {
            turn: turn.clone(),
            prompt: prompt.clone(),
            config: config.clone(),
        });
        Ok(turn)
    }

    async fn steer_turn(&mut self, turn: &str, text: &str) -> Result<(), AgentSessionError> {
        self.steer_input(turn, &AgentPrompt::text(text)).await
    }

    async fn steer_input(
        &mut self,
        turn: &str,
        prompt: &AgentPrompt,
    ) -> Result<(), AgentSessionError> {
        let native = self
            .turns
            .as_ref()
            .filter(|(logical, _)| logical == turn)
            .map_or(turn, |(_, native)| native)
            .to_owned();
        self.inner()?.steer_input(&native, prompt).await
    }

    async fn cancel_turn(&mut self, turn: &str) -> Result<(), AgentSessionError> {
        if self.closed {
            return Ok(());
        }
        if self.input.as_ref().is_some_and(|input| input.turn == turn) {
            self.close().await?;
            self.events.push_back(AgentTurnEvent::Cancelled);
            return Ok(());
        }
        let native = self
            .turns
            .as_ref()
            .filter(|(logical, _)| logical == turn)
            .map_or(turn, |(_, native)| native)
            .to_owned();
        self.inner()?.cancel_turn(&native).await
    }

    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
        if self.failed {
            return Err(AgentSessionError::Rejected);
        }
        if let Some(inner) = &mut self.inner {
            return inner.respond_permission(id, response).await;
        }
        let native = self.dialogs.response(id, response)?;
        self.writer
            .as_ref()
            .ok_or(AgentSessionError::Unavailable)?
            .send(&native)
            .await?;
        self.dialogs.remove(id);
        self.events
            .push_back(AgentTurnEvent::PermissionResolved(id.to_owned()));
        Ok(())
    }

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if self.closed || self.failed {
            return Ok(self.events.pop_front());
        }
        while let Ok(frame) = self.receiver.try_recv() {
            if let Err(error) = self.frame(frame) {
                self.fail(error);
                return Ok(self.events.pop_front());
            }
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        let event = match self.inner.as_mut().map(AgentSession::poll_turn) {
            Some(Ok(event)) => event,
            Some(Err(error)) => {
                self.fail(error);
                return Ok(self.events.pop_front());
            }
            None => None,
        };
        let event = event.map(|event| self.event(event));
        if matches!(
            event,
            Some(AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed)
        ) {
            self.turns.take();
        }
        Ok(event)
    }

    async fn close(&mut self) -> Result<(), AgentSessionError> {
        self.closed = true;
        self.receiver.close();
        self.input.take();
        if let Some(task) = self.task.take() {
            task.abort();
            // Join cancellation so the startup future drops its transport before close returns.
            let _ = task.await;
        }
        self.writer.take();
        while self.receiver.try_recv().is_ok() {}
        self.ready.close();
        if let Ok(Ok(mut inner)) = self.ready.try_recv() {
            // Startup may have finished before cancellation without advance adopting its result.
            // Close that native process without publishing its identity or replaying queued input.
            inner.close().await?;
        }
        self.events.extend(self.dialogs.clear());
        if let Some(inner) = &mut self.inner {
            inner.close().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
