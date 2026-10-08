use std::collections::BTreeMap;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    AntigravityClient, PROVIDER, config, diagnostics::Failure, streaming::Stream,
    transport::Transport,
};
use crate::ports::agent_session::{
    AgentSession, AgentSessionError, AgentSessionFuture, AgentSessionSpec, AgentTurnEvent,
};
use crate::protocol::prompt::AgentPrompt;

#[derive(Debug)]
pub(super) struct Session {
    client: AntigravityClient,
    transport: Option<Transport>,
    id: String,
    cwd: String,
    config: StoredAgentConfig,
    init: Value,
    active: Option<String>,
    stream: Stream,
    closed: bool,
}

pub(super) async fn open(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    handle: Option<&AgentPersistenceHandle>,
) -> Result<Session, AgentSessionError> {
    config::validate_spec(spec)?;
    if let Some(handle) = handle
        && (handle.provider != PROVIDER
            || Uuid::parse_str(&handle.session_id).is_err()
            || handle
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cwd"))
                .and_then(Value::as_str)
                != Some(&spec.cwd))
    {
        return Err(AgentSessionError::Rejected);
    }
    let conversation = handle.map(|handle| handle.session_id.as_str());
    let (transport, id, init) = connect(client, spec, conversation).await?;
    Ok(Session {
        client: client.clone(),
        transport: Some(transport),
        id,
        cwd: spec.cwd.clone(),
        config: spec.config.clone(),
        init,
        active: None,
        stream: Stream::default(),
        closed: false,
    })
}

async fn connect(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    conversation: Option<&str>,
) -> Result<(Transport, String, Value), AgentSessionError> {
    let mut transport = Transport::spawn(
        client,
        &spec.cwd,
        &config::arguments(&spec.config, conversation),
    )?;
    let response = tokio::time::timeout(client.deadline, transport.receive()).await;
    let Ok(Ok(initialized)) = response else {
        let _ = transport.close().await;
        tracing::warn!(
            message = transport
                .failure()
                .unwrap_or(if response.is_err() {
                    Failure::Timeout
                } else {
                    Failure::Exit
                })
                .message(),
            "AGY initialization failed"
        );
        return Err(AgentSessionError::Failed);
    };
    let id = initialized["conversation_id"]
        .as_str()
        .ok_or(AgentSessionError::Failed)?;
    if initialized["event"] != "init"
        || Uuid::parse_str(id).is_err()
        || conversation.is_some_and(|expected| expected != id)
        || initialized["init"]["cwd"] != spec.cwd
        || !initialized["init"]["permission_mode"].is_string()
        || spec
            .config
            .model
            .as_ref()
            .is_some_and(|model| initialized["init"]["model"] != *model)
    {
        return Err(AgentSessionError::Failed);
    }
    Ok((transport, id.to_owned(), initialized["init"].clone()))
}

impl Session {
    async fn start(
        &mut self,
        prompt: &AgentPrompt,
        config: &StoredAgentConfig,
    ) -> Result<String, AgentSessionError> {
        prompt.validate()?;
        config::validate(config)?;
        if self.closed
            || self.active.is_some()
            || !prompt.images.is_empty()
            || prompt.output_schema.is_some()
        {
            return Err(AgentSessionError::Rejected);
        }
        let blocks = prompt.blocks()?;
        self.stream.failure = None;
        if self.transport.is_none() || self.config != *config {
            if let Some(mut transport) = self.transport.take() {
                transport.close().await?;
            }
            let spec = AgentSessionSpec {
                provider: PROVIDER.to_owned(),
                cwd: self.cwd.clone(),
                config: config.clone(),
            };
            let (transport, _, init) = connect(&self.client, &spec, Some(&self.id)).await?;
            self.transport = Some(transport);
            self.init = init;
            self.config.clone_from(config);
        }
        // Write once; a timeout leaves admission uncertain and closes the writer.
        let sent = self
            .transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?
            .send(&json!({"event":"user","message":{"content":blocks}}))
            .await;
        if let Err(error) = sent {
            self.stream.failure = Some(
                self.transport
                    .as_ref()
                    .and_then(Transport::failure)
                    .unwrap_or(Failure::Exit),
            );
            return Err(error);
        }
        let turn = Uuid::new_v4().to_string();
        self.stream.begin(turn.clone(), self.id.clone());
        self.active = Some(turn.clone());
        self.stream
            .events
            .push_back(AgentTurnEvent::RuntimeInfo(config::runtime(
                &self.id,
                &self.config,
                &self.init,
            )));
        Ok(turn)
    }

    fn consume(&mut self, message: &Value) -> Result<(), AgentSessionError> {
        if self.active.is_none() {
            return Err(AgentSessionError::Failed);
        }
        match message["event"].as_str() {
            Some("step_update") => self.stream.update(&message["step_update"]),
            Some("result") => {
                self.stream.finish(&message["result"])?;
                self.active = None;
                Ok(())
            }
            Some("init") | None => Err(AgentSessionError::Failed),
            Some(_) => Ok(()),
        }
    }
}

impl AgentSession for Session {
    fn failure_message(&self) -> Option<&str> {
        self.stream
            .failure
            .into_iter()
            .chain(self.transport.as_ref().and_then(Transport::failure))
            .max()
            .map(Failure::message)
    }

    fn provider(&self) -> &'static str {
        PROVIDER
    }

    fn runtime_info(&mut self) -> AgentSessionFuture<'_, StoredAgentRuntimeInfo> {
        Box::pin(async { Ok(config::runtime(&self.id, &self.config, &self.init)) })
    }

    fn persistence(&self) -> Option<AgentPersistenceHandle> {
        Some(AgentPersistenceHandle {
            provider: PROVIDER.to_owned(),
            session_id: self.id.clone(),
            native_handle: None,
            metadata: Some(BTreeMap::from([("cwd".to_owned(), json!(self.cwd))])),
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

    fn poll_turn(&mut self) -> Result<Option<AgentTurnEvent>, AgentSessionError> {
        if let Some(event) = self.stream.events.pop_front() {
            return Ok(Some(event));
        }
        let Some(transport) = &mut self.transport else {
            return Ok(None);
        };
        let message = match transport.poll() {
            Ok(Some(message)) => message,
            Ok(None) => return Ok(None),
            Err(_) => {
                self.stream
                    .abort(transport.failure().unwrap_or(Failure::Exit));
                self.active = None;
                return Ok(self.stream.events.pop_front());
            }
        };
        if self.consume(&message).is_err() {
            let failure = self
                .transport
                .as_ref()
                .and_then(Transport::failure)
                .unwrap_or(Failure::Protocol);
            self.stream.abort(failure);
            self.active = None;
        }
        Ok(self.stream.events.pop_front())
    }

    fn cancel_turn<'a>(&'a mut self, turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.active.as_deref() != Some(turn_id) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            self.transport
                .as_ref()
                .ok_or(AgentSessionError::Failed)?
                .interrupt()
                .await?;
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                while self.active.is_some() {
                    let message = self
                        .transport
                        .as_mut()
                        .ok_or(AgentSessionError::Failed)?
                        .receive()
                        .await?;
                    self.consume(&message)?;
                }
                if self
                    .stream
                    .events
                    .iter()
                    .any(|event| matches!(event, AgentTurnEvent::Cancelled))
                {
                    Ok(())
                } else {
                    Err(AgentSessionError::Rejected)
                }
            })
            .await;
            let stopped = if let Some(mut transport) = self.transport.take() {
                transport.close().await
            } else {
                Ok(())
            };
            stopped?;
            match result {
                Ok(result) => result,
                Err(_) => Err(AgentSessionError::Failed),
            }
        })
    }

    fn close(&mut self) -> AgentSessionFuture<'_, ()> {
        Box::pin(async move {
            let failed = self.stream.failure.is_some() || self.active.is_some();
            if let Some(mut transport) = self.transport.take() {
                transport.close().await?;
                if let Some(failure) = transport.failure().filter(|_| failed) {
                    self.stream.failure = Some(
                        self.stream
                            .failure
                            .map_or(failure, |current| current.max(failure)),
                    );
                }
            }
            if let Some(failure) = self.stream.failure {
                tracing::warn!(message = failure.message(), "AGY session failed");
            }
            self.closed = true;
            Ok(())
        })
    }
}
