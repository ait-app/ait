//! Publish standard startup interaction before waiting for the external runtime's initialization.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tokio_util::task::AbortOnDropHandle;

use super::{OmpClient, PROVIDER, bridge, config, interactions::Dialogs, transport::Writer, ui};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

/// Startup frames retain native request identity and use the same single serialized writer.
#[derive(Debug)]
pub(super) enum Frame {
    Writer(Writer),
    Ui(Value),
}

type Ready = Result<bridge::Session, AgentSessionError>;
type SideCompletion = (
    oneshot::Receiver<Result<(), AgentSessionError>>,
    AbortOnDropHandle<()>,
);

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
    inner: Option<bridge::Session>,
    writer: Option<Writer>,
    dialogs: Dialogs,
    ui: ui::State,
    events: VecDeque<AgentTurnEvent>,
    input: Option<Input>,
    turns: Option<(String, String)>,
    side: Option<SideCompletion>,
    closed: bool,
}

/// Keep normal initialization unchanged; return early only to let the user answer a startup dialog.
/// Invalid native initialization remains an error, and cancellation never replays input.
pub(super) async fn open(
    client: &OmpClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Box<dyn AgentSession>, AgentSessionError> {
    config::validate_spec(spec)?;
    let (sender, receiver) = mpsc::channel(128);
    let (finished, ready) = oneshot::channel();
    let mut client = client.clone();
    client.startup = Arc::new(Mutex::new(Some(sender)));
    let launch = spec.clone();
    let restore = handle.cloned();
    let task = AbortOnDropHandle::new(tokio::spawn(async move {
        let result = bridge::Session::open(&client, &launch, restore.as_ref()).await;
        if let Ok(mut startup) = client.startup.lock() {
            startup.take();
        }
        let _ = finished.send(result);
    }));
    let mut session = Session {
        receiver,
        ready,
        task: Some(task),
        inner: None,
        writer: None,
        dialogs: Dialogs::default(),
        ui: ui::State::default(),
        events: VecDeque::new(),
        input: None,
        turns: None,
        side: None,
        closed: false,
    };
    loop {
        tokio::select! {
            ready = &mut session.ready => {
                let mut inner = ready.map_err(|_| AgentSessionError::Failed)??;
                inner.adopt_startup(session.dialogs, session.ui)?;
                return Ok(Box::new(inner));
            }
            frame = session.receiver.recv() => {
                let Some(frame) = frame else {
                    let mut inner=session.ready.await.map_err(|_|AgentSessionError::Failed)??;
                    inner.adopt_startup(session.dialogs,session.ui)?;
                    return Ok(Box::new(inner));
                };
                session.frame(frame)?;
                if !session.dialogs.pending().is_empty() { return Ok(Box::new(session)); }
            }
        }
    }
}

impl Session {
    fn frame(&mut self, frame: Frame) -> Result<(), AgentSessionError> {
        match frame {
            Frame::Writer(writer) => self.writer = Some(writer),
            Frame::Ui(frame) => {
                self.ui.capture(&frame)?;
                self.events.extend(self.dialogs.capture(&frame)?);
            }
        }
        Ok(())
    }

    async fn advance(&mut self) -> Result<(), AgentSessionError> {
        while let Ok(frame) = self.receiver.try_recv() {
            self.frame(frame)?;
        }
        if self.inner.is_none() && !self.closed {
            match self.ready.try_recv() {
                Ok(result) => {
                    let mut inner = result?;
                    inner.adopt_startup(
                        std::mem::take(&mut self.dialogs),
                        std::mem::take(&mut self.ui),
                    )?;
                    self.writer.take();
                    self.task.take();
                    self.events
                        .push_back(AgentTurnEvent::RuntimeInfo(inner.runtime_info().await?));
                    self.inner = Some(inner);
                }
                Err(oneshot::error::TryRecvError::Empty) => return Ok(()),
                Err(oneshot::error::TryRecvError::Closed) => return Err(AgentSessionError::Failed),
            }
        }
        if let Some(inner) = &mut self.inner
            && let Some(input) = self.input.take()
        {
            if inner.handles_out_of_band(&input.prompt.text) == Some(true) {
                inner.out_of_band(&input.prompt).await?;
                let (done, completed) = oneshot::channel();
                let completion = inner.take_command_completion();
                let task = AbortOnDropHandle::new(tokio::spawn(async move {
                    let result = if let Some(completion) = completion {
                        completion.await
                    } else {
                        Ok(())
                    };
                    let _ = done.send(result);
                }));
                self.side = Some((completed, task));
                self.turns = Some((input.turn, String::new()));
            } else {
                let turn = inner.start_input(&input.prompt, &input.config).await?;
                self.turns = Some((input.turn, turn));
            }
        }
        Ok(())
    }

    fn inner(&mut self) -> Result<&mut bridge::Session, AgentSessionError> {
        self.inner.as_mut().ok_or(AgentSessionError::Busy)
    }

    fn event(&mut self, event: AgentTurnEvent) -> AgentTurnEvent {
        let Some((logical, native)) = &self.turns else {
            return event;
        };
        match event {
            AgentTurnEvent::Started(turn) if &turn == native => {
                AgentTurnEvent::Started(logical.clone())
            }
            AgentTurnEvent::Timeline(mut entry) => {
                if entry.turn_id.as_ref() == Some(native) {
                    entry.turn_id = Some(logical.clone());
                }
                AgentTurnEvent::Timeline(entry)
            }
            AgentTurnEvent::Progress {
                observation,
                mut entry,
            } => {
                if entry.turn_id.as_ref() == Some(native) {
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
    fn provider(&self) -> &str {
        PROVIDER
    }
    fn handles_out_of_band(&self, text: &str) -> Option<bool> {
        self.inner
            .as_ref()
            .map_or(Some(false), |inner| inner.handles_out_of_band(text))
    }
    fn commands(&self) -> Option<&[Value]> {
        self.inner.as_ref().and_then(AgentSession::commands)
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        self.inner.as_ref().and_then(AgentSession::persistence)
    }
    fn control_settings(&self, config: &StoredAgentConfig) -> Option<Value> {
        self.inner
            .as_ref()
            .and_then(|inner| inner.control_settings(config))
    }
    fn ui_state(&self) -> Option<Value> {
        Some(
            self.inner
                .as_ref()
                .and_then(AgentSession::ui_state)
                .unwrap_or_else(|| self.ui.snapshot()),
        )
    }
    fn validate_config_update(&self, config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
        self.inner.as_ref().map_or_else(
            || config::validate(config),
            |inner| inner.validate_config_update(config),
        )
    }
    fn observation_committed(&mut self) {
        if let Some(inner) = &mut self.inner {
            inner.observation_committed();
        }
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
    fn take_command_completion(&mut self) -> Option<AgentSessionFuture<'static, ()>> {
        self.inner
            .as_mut()
            .and_then(AgentSession::take_command_completion)
    }
    fn failure_diagnostic(&self) -> Option<crate::ports::diagnostics::Diagnostic> {
        self.inner
            .as_ref()
            .and_then(AgentSession::failure_diagnostic)
    }
    fn failure_message(&self) -> Option<Cow<'_, str>> {
        self.inner.as_ref().and_then(AgentSession::failure_message)
    }
    async fn runtime_info(&mut self) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
        if let Some(inner) = &mut self.inner {
            return inner.runtime_info().await;
        }
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
        if self.closed {
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
        if self.input.as_ref().is_some_and(|input| input.turn == turn) {
            self.close().await?;
            self.events.push_back(AgentTurnEvent::Cancelled);
            return Ok(());
        }
        if self
            .turns
            .as_ref()
            .is_some_and(|(logical, native)| logical == turn && native.is_empty())
        {
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
    async fn out_of_band(&mut self, prompt: &AgentPrompt) -> Result<(), AgentSessionError> {
        self.inner()?.out_of_band(prompt).await
    }
    async fn begin_action(
        &mut self,
        action: &str,
        args: &Value,
    ) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
        self.inner()?.begin_action(action, args).await
    }
    async fn begin_configured_action(
        &mut self,
        action: &str,
        args: &Value,
        config: &StoredAgentConfig,
    ) -> Result<AgentSessionFuture<'static, Value>, AgentSessionError> {
        self.inner()?
            .begin_configured_action(action, args, config)
            .await
    }
    async fn complete_commands(
        &mut self,
        input: &crate::protocol::commands::CompletionInput,
    ) -> Result<crate::protocol::commands::ValidatedCompletion, AgentSessionError> {
        self.inner()?.complete_commands(input).await
    }
    async fn respond_permission(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<(), AgentSessionError> {
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
        if self.closed {
            return Ok(self.events.pop_front());
        }
        while let Ok(frame) = self.receiver.try_recv() {
            self.frame(frame)?;
        }
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        if let Some((receiver, _)) = &mut self.side {
            match receiver.try_recv() {
                Ok(result) => {
                    self.side.take();
                    self.turns.take();
                    return Ok(Some(if result.is_ok() {
                        AgentTurnEvent::Completed(None)
                    } else {
                        AgentTurnEvent::Failed
                    }));
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => return Err(AgentSessionError::Failed),
            }
        }
        let event = self
            .inner
            .as_mut()
            .map(AgentSession::poll_turn)
            .transpose()?
            .flatten();
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
        while self.receiver.try_recv().is_ok() {}
        self.input.take();
        self.side.take();
        self.task.take();
        self.writer.take();
        self.events.extend(self.dialogs.clear());
        if let Some(inner) = &mut self.inner {
            inner.close().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
