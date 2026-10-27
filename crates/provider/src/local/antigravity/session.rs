use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::Duration;

use domain::agent_runtime::{AgentPersistenceHandle, StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{
    AntigravityClient, PROVIDER, config, diagnostics::Failure, discovery, input, streaming::Stream,
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
    input_error: Option<&'static str>,
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
        input_error: None,
        closed: false,
    })
}

async fn connect(
    client: &AntigravityClient,
    spec: &AgentSessionSpec,
    conversation: Option<&str>,
) -> Result<(Transport, String, Value), AgentSessionError> {
    // Session startup can launch OAuth. Check cached authentication through the read-only model
    // command first, including automatic restore and configuration-triggered reconnects.
    discovery::models(client, &spec.cwd).await?;
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
        if self.closed || self.active.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        self.input_error = None;
        config::validate(config).inspect_err(|_| {
            self.input_error = Some("AGY does not support the requested session configuration.");
        })?;
        if prompt.output_schema.is_some() {
            self.input_error = Some(
                "AGY does not support JSON schema output constraints. Remove the output schema and retry.",
            );
            return Err(AgentSessionError::Rejected);
        }
        let blocks = prompt.blocks().inspect_err(|_| {
            self.input_error = Some("AGY input is empty, invalid or too large. Check the text and image attachments before retrying.");
        })?;
        let input = input::prepare(blocks, &self.client.images).inspect_err(|_| {
            self.input_error = Some("Ait could not save AGY image attachments. Check disk space and provider image directory permissions before retrying.");
        })?;
        self.stream.failure = None;
        self.stream.explanation.clear();
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
            self.stream.reset_process();
        }
        // Write once; a timeout leaves admission uncertain and closes the writer.
        let sent = self
            .transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?
            .send(&json!({"event":"user","message":{"content":input.content}}))
            .await;
        if let Err(error) = sent {
            self.stream.failure = Some(
                self.transport
                    .as_ref()
                    .and_then(Transport::failure)
                    .unwrap_or(Failure::Exit),
            );
            self.transport.take();
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
        self.stream.submitted(
            &input.text,
            &input.sources,
            prompt.client_message_id.as_deref(),
        );
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
                // Native ERROR (including interrupted) retires the driver. A later explicit
                // prompt resumes the conversation; never retry an ambiguously admitted prompt.
                if message["result"]["status"] != "SUCCESS" {
                    self.transport.take();
                }
                Ok(())
            }
            Some("init") | None => Err(AgentSessionError::Failed),
            Some(_) => Ok(()),
        }
    }
}

impl AgentSession for Session {
    fn account_usage(&self) -> AgentSessionFuture<'_, Value> {
        Box::pin(super::quota::read(
            &self.client,
            std::path::Path::new(&self.cwd),
        ))
    }

    fn failure_diagnostic(&self) -> Option<crate::ports::diagnostics::Diagnostic> {
        use crate::ports::diagnostics::{Category, Diagnostic};
        if self.input_error.is_some() {
            return Some(Diagnostic::new(Category::Unknown, "input"));
        }
        let failure = self
            .stream
            .failure
            .into_iter()
            .chain(self.transport.as_ref().and_then(Transport::failure))
            .max()?;
        let category = match failure {
            Failure::Exit => Category::Closed,
            Failure::Protocol => Category::Protocol,
            Failure::Timeout => Category::Timeout,
            Failure::Native => Category::Unknown,
            Failure::Authentication => Category::Authentication,
            Failure::Quota => Category::Quota,
            Failure::Permission => Category::Permission,
        };
        Some(Diagnostic::new(category, "result"))
    }

    fn failure_message(&self) -> Option<Cow<'_, str>> {
        if let Some(message) = self.input_error {
            return Some(Cow::Borrowed(message));
        }
        if self.stream.failure != Some(Failure::Permission)
            && let Some(message) = self.stream.explanation.message()
        {
            return Some(Cow::Borrowed(message));
        }
        self.stream
            .failure
            .into_iter()
            .chain(self.transport.as_ref().and_then(Transport::failure))
            .max()
            .map(|failure| Cow::Borrowed(failure.message()))
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
                let failure = transport.failure().unwrap_or(Failure::Exit);
                self.transport.take();
                if self.active.take().is_some() {
                    self.stream.abort(failure);
                }
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
            self.transport.take();
        }
        Ok(self.stream.events.pop_front())
    }

    fn cancel_turn<'a>(&'a mut self, turn_id: &'a str) -> AgentSessionFuture<'a, ()> {
        Box::pin(async move {
            if self.active.as_deref() != Some(turn_id) || self.closed {
                return Err(AgentSessionError::Rejected);
            }
            let result = tokio::time::timeout(Duration::from_secs(5), async {
                self.transport
                    .as_ref()
                    .ok_or(AgentSessionError::Failed)?
                    .interrupt()
                    .await?;
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
            let fallback = if result.is_err() {
                Failure::Timeout
            } else {
                Failure::Exit
            };
            let failure = self
                .transport
                .as_ref()
                .and_then(Transport::failure)
                .unwrap_or(fallback);
            let stopped = if let Some(mut transport) = self.transport.take() {
                transport.close().await
            } else {
                Ok(())
            };
            // An unacknowledged cancellation is a failure, but cannot leave a turn active
            // after its transport has been closed. Emit exactly one terminal event.
            if self.active.take().is_some() {
                self.stream.abort(failure);
            }
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
