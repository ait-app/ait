//! One foreground input at a time; observation runs independently of the server actor.
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::Arc,
    time::Duration,
};

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};

use super::{
    bridge::Bridge,
    client,
    http::{Api, Version},
    projection,
    session::{Connection, Submission},
    types::{Fault, Outcome, ProtocolError, Snapshot},
};
use crate::{
    ports::agent_session::{AgentSession, AgentSessionError, AgentSessionFuture, AgentTurnEvent},
    protocol::{prompt::AgentPrompt, timeline::NativeItem},
};

type Finished = (Connection, Result<Snapshot, ProtocolError>, bool);

pub(super) struct Session {
    connection: Option<Connection>,
    finished: Option<oneshot::Receiver<Finished>>,
    task: Option<AbortOnDropHandle<()>>,
    cancel: CancellationToken,
    api: Api,
    cancel_acknowledged: Arc<std::sync::atomic::AtomicBool>,
    events: mpsc::Receiver<AgentTurnEvent>,
    sender: mpsc::Sender<AgentTurnEvent>,
    queued: VecDeque<AgentTurnEvent>,
    bridge: Option<Arc<Bridge>>,
    config: StoredAgentConfig,
    info: StoredAgentRuntimeInfo,
    clients: BTreeMap<String, String>,
    known: BTreeMap<String, NativeItem>,
    turn: Option<String>,
    history_only: bool,
    closed: bool,
    failed: bool,
}

impl fmt::Debug for Session {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("OpenCodeSession")
            .field("info", &self.info)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(super) fn new(
        connection: Connection,
        config: &StoredAgentConfig,
        clients: BTreeMap<String, String>,
        history_only: bool,
    ) -> Result<Self, AgentSessionError> {
        let (sender, events) = mpsc::channel(256);
        let snapshot = &connection.prepared;
        let config = client::effective(config, &snapshot.model);
        let info = StoredAgentRuntimeInfo {
            provider: "opencode".into(),
            session_id: Some(snapshot.id.clone()),
            model: Some(snapshot.model.clone()),
            thinking_option_id: snapshot.reasoning_effort.clone(),
            mode_id: Some("build".into()),
            extra: None,
        };
        let known = projection::entries(snapshot, &clients)?
            .into_iter()
            .map(|item| (item.key.clone(), item))
            .collect();
        Ok(Self {
            cancel: connection.invocation.cancellation.clone(),
            api: connection.runtime.api.clone(),
            cancel_acknowledged: connection.invocation.cancel_acknowledged.clone(),
            connection: Some(connection),
            finished: None,
            task: None,
            events,
            sender,
            queued: VecDeque::new(),
            bridge: None,
            config,
            info,
            clients,
            known,
            turn: None,
            history_only,
            closed: false,
            failed: false,
        })
    }

    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        client::validate(config)?;
        if !prompt.images.is_empty()
            || !prompt.attachments.is_empty()
            || prompt.output_schema.is_some()
            || self.closed
            || self.failed
            || self.history_only
            || self.turn.is_some()
            || client::effective(
                config,
                self.info
                    .model
                    .as_deref()
                    .ok_or(AgentSessionError::Failed)?,
            ) != self.config
            || self.clients.len() >= 512
        {
            return Err(AgentSessionError::Rejected);
        }
        let connection = self.connection.as_mut().ok_or(AgentSessionError::Failed)?;
        let turn = input_id(connection.runtime.api.version);
        connection.invocation.input_id.clone_from(&turn);
        connection.prepared.input_id.clone_from(&turn);
        connection.invocation.prompt.clone_from(&prompt.text);
        connection
            .invocation
            .instructions
            .clone_from(&config.system_prompt);
        connection.invocation.cancellation = CancellationToken::new();
        self.cancel = connection.invocation.cancellation.clone();
        self.cancel_acknowledged
            .store(false, std::sync::atomic::Ordering::Release);
        connection.submitted = false;
        let bridge = Arc::new(Bridge::new(
            self.sender.clone(),
            connection.runtime.api.version,
            turn.clone(),
            prompt.client_message_id.clone(),
        ));
        connection.invocation.approvals = bridge.clone();
        if let Some(client) = &prompt.client_message_id {
            self.clients.insert(turn.clone(), client.clone());
        }
        // Await one native admission response; an ambiguous failure permanently poisons this writer.
        let submitted = match connection.submit().await {
            Ok(value) => value,
            Err(error) => {
                self.failed = connection.submitted;
                return Err(client::error(error));
            }
        };
        let connection = self.connection.take().ok_or(AgentSessionError::Failed)?;
        let (sender, finished) = oneshot::channel();
        let progress = bridge.clone();
        self.task = Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let mut result = match submitted {
                Submission::Settled(snapshot) => Ok(snapshot),
                Submission::Observing(events) => connection.observe(events, progress.clone()).await,
            };
            let cancelled = result
                .as_ref()
                .is_err_and(|error| error.code == Fault::RunCancelled);
            if cancelled {
                result = reconcile_interrupt(&connection).await;
            }
            progress.clear().await;
            let _ = sender.send((connection, result, cancelled));
        })));
        self.finished = Some(finished);
        self.bridge = Some(bridge);
        self.turn = Some(turn.clone());
        Ok(turn)
    }

    fn finish(
        &mut self,
        mut connection: Connection,
        result: Result<Snapshot, ProtocolError>,
        cancelled: bool,
    ) -> Result<(), AgentSessionError> {
        self.task = None;
        self.finished = None;
        match result {
            Ok(snapshot) => {
                let entries = projection::entries(&snapshot, &self.clients)?;
                for entry in &entries {
                    if self
                        .known
                        .get(&entry.key)
                        .is_some_and(|previous| previous != entry)
                    {
                        self.failed = true;
                        return Err(AgentSessionError::Failed);
                    }
                }
                let last = entries
                    .iter()
                    .rev()
                    .find(|entry| entry.item["type"] == "assistant_message")
                    .and_then(|entry| entry.item["text"].as_str())
                    .map(str::to_owned);
                for entry in entries {
                    if !self.known.contains_key(&entry.key) {
                        self.known.insert(entry.key.clone(), entry.clone());
                        self.queued.push_back(AgentTurnEvent::Timeline(entry));
                    }
                }
                self.queued.push_back(if cancelled {
                    AgentTurnEvent::Cancelled
                } else {
                    match snapshot.outcome {
                        Some(Outcome::Completed) => AgentTurnEvent::Completed(last),
                        Some(Outcome::Interrupted) => AgentTurnEvent::Cancelled,
                        Some(Outcome::Failed) | None => AgentTurnEvent::Failed,
                    }
                });
                connection.prepared = snapshot;
            }
            Err(error) => {
                self.failed = true;
                self.queued.push_back(if error.code == Fault::RunCancelled {
                    AgentTurnEvent::Cancelled
                } else {
                    AgentTurnEvent::Failed
                });
            }
        }
        self.connection = Some(connection);
        Ok(())
    }
}

impl AgentSession for Session {
    fn provider(&self) -> &'static str {
        "opencode"
    }
    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async {
            if self.closed {
                Err(AgentSessionError::Unavailable)
            } else {
                Ok(self.info.clone())
            }
        })
    }
    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: "opencode".into(),
            session_id: self.info.session_id.clone()?,
            native_handle: Some(json!(
                json!({"config":self.config,"model":self.info.model,"clients":self.clients})
                    .to_string()
            )),
            metadata: None,
        })
    }
    fn start_turn<'a>(
        &'a mut self,
        text: &'a str,
        config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(async move { self.start(&AgentPrompt::text(text), config).await })
    }
    fn start_input<'a>(
        &'a mut self,
        prompt: &'a AgentPrompt,
        config: &'a StoredAgentConfig,
    ) -> AgentSessionFuture<'a, String> {
        Box::pin(self.start(prompt, config))
    }
    fn pending_permissions(&self) -> Vec<Value> {
        self.bridge
            .as_ref()
            .map_or_else(Vec::new, |bridge| bridge.pending())
    }
    fn respond_permission<'a>(
        &'a mut self,
        id: &'a str,
        response: &'a Value,
    ) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            self.bridge
                .as_ref()
                .ok_or(AgentSessionError::Rejected)?
                .respond(id, response)
        })
    }
    fn cancel_turn<'a>(&'a mut self, turn: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.turn.as_deref() != Some(turn) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            if self
                .task
                .as_ref()
                .is_some_and(AbortOnDropHandle::is_finished)
            {
                return Err(AgentSessionError::Rejected);
            }
            let suffix = if self.api.version == Version::V1 {
                "/abort"
            } else {
                "/interrupt"
            };
            let id = self
                .info
                .session_id
                .as_deref()
                .ok_or(AgentSessionError::Failed)?;
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                self.api.json(
                    reqwest::Method::POST,
                    &self.api.path(id, suffix),
                    Some(&json!({})),
                ),
            )
            .await;
            if !result.is_ok_and(|result| result.is_ok()) {
                self.cancel.cancel();
                return Err(AgentSessionError::Failed);
            }
            self.cancel_acknowledged
                .store(true, std::sync::atomic::Ordering::Release);
            self.cancel.cancel();
            Ok(())
        })
    }
    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Ok(event) = self.events.try_recv() {
            return Ok(Some(event));
        }
        if let Some(receiver) = &mut self.finished {
            match receiver.try_recv() {
                Ok((connection, result, cancelled)) => {
                    self.finish(connection, result, cancelled)?;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.failed = true;
                    return Err(AgentSessionError::Failed);
                }
            }
        }
        let event = self.queued.pop_front();
        if matches!(
            event,
            Some(AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed)
        ) {
            self.turn = None;
        }
        Ok(event)
    }
    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            self.closed = true;
            self.cancel.cancel();
            // Drain display backpressure while the owned observer acknowledges interruption.
            self.events.close();
            let mut uncertain = false;
            if let Some(mut task) = self.task.take()
                && tokio::time::timeout(Duration::from_secs(6), &mut task)
                    .await
                    .is_err()
            {
                task.abort();
                let _ = tokio::time::timeout(Duration::from_secs(2), &mut task).await;
                uncertain = true;
            }
            if let Some(mut receiver) = self.finished.take()
                && let Ok((connection, _, _)) = receiver.try_recv()
            {
                self.connection = Some(connection);
            }
            if let Some(mut connection) = self.connection.take() {
                connection.runtime.close().await.map_err(client::error)?;
            }
            if uncertain {
                Err(AgentSessionError::Failed)
            } else {
                Ok(())
            }
        })
    }
}

async fn reconcile_interrupt(connection: &Connection) -> Result<Snapshot, ProtocolError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match super::session::snapshot(
                &connection.runtime.api,
                &connection.prepared.id,
                &connection.invocation,
            )
            .await
            {
                Ok(snapshot) => return Ok(snapshot),
                Err(error)
                    if matches!(error.code, Fault::SessionBusy | Fault::RunRecoveryFailed) =>
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| {
        super::failure(
            Fault::RunRecoveryFailed,
            "interrupted history did not drain",
        )
    })?
}

fn input_id(version: Version) -> String {
    let random = uuid::Uuid::new_v4().simple().to_string();
    match version {
        Version::V1 => {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            format!(
                "msg_{:012x}{}",
                (timestamp << 12) & 0xffff_ffff_ffff,
                &random[..14]
            )
        }
        Version::V2 => random,
    }
}

#[cfg(test)]
mod tests;
