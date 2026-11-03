//! Native ACP command catalogs, independent of prompt templates and TUI-local actions.

use std::collections::VecDeque;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::watch;

use super::{OpenCodeClient, launcher, summary::Temporary};
use crate::local::acp_transport::Transport;
use crate::ports::agent_session::{AgentSessionError, AgentSessionSpec};

/// Session-owned snapshot with updates from the active ACP reader.
#[derive(Debug)]
pub(super) struct Catalog {
    items: Option<Vec<Value>>,
    sender: watch::Sender<Option<Vec<Value>>>,
    receiver: watch::Receiver<Option<Vec<Value>>>,
}

impl Catalog {
    /// Create an unknown catalog, distinct from a native empty directory.
    pub(super) fn new() -> Self {
        let (sender, receiver) = watch::channel(None);
        Self {
            items: None,
            sender,
            receiver,
        }
    }

    /// Share updates with the same connection's foreground reader.
    pub(super) fn sender(&self) -> watch::Sender<Option<Vec<Value>>> {
        self.sender.clone()
    }

    /// Last authoritative native catalog, if the runtime has supplied one.
    pub(super) fn items(&self) -> Option<&[Value]> {
        self.items.as_deref()
    }

    /// Observe an update published by the foreground task without blocking execution.
    pub(super) fn observe(&mut self) {
        if self.receiver.has_changed().unwrap_or(false) {
            self.items.clone_from(&self.receiver.borrow_and_update());
        }
    }

    /// Read queued idle updates, retaining other frames for the foreground owner.
    /// Invalid native catalogs or exceeded deferred-frame budgets return a failure.
    pub(super) fn drain(
        &mut self,
        id: &str,
        transport: &mut Transport,
        deferred: &mut VecDeque<Value>,
    ) -> Result<(), AgentSessionError> {
        for _ in 0..128 {
            let Some(frame) = transport.poll()? else {
                break;
            };
            self.ingest(id, frame, deferred)?;
        }
        self.observe();
        Ok(())
    }

    fn ingest(
        &self,
        id: &str,
        frame: Value,
        deferred: &mut VecDeque<Value>,
    ) -> Result<(), AgentSessionError> {
        if !publish(&self.sender, id, &frame)? {
            if deferred.len() >= 4096 {
                return Err(AgentSessionError::Failed);
            }
            deferred.push_back(frame);
        }
        Ok(())
    }

    /// Await the first post-response ACP catalog, with no prompt or replacement native writer.
    /// Timeout means the runtime did not advertise a catalog; malformed updates remain errors.
    pub(super) async fn refresh(
        &mut self,
        id: &str,
        connection: Option<(&mut Transport, &mut VecDeque<Value>)>,
    ) -> Result<Option<Vec<Value>>, AgentSessionError> {
        if let Some((transport, deferred)) = connection {
            self.drain(id, transport, deferred)?;
            if self.items.is_none() {
                let result = tokio::time::timeout(Duration::from_millis(1500), async {
                    while self.items.is_none() {
                        let frame = transport.receive().await?;
                        self.ingest(id, frame, deferred)?;
                        self.observe();
                    }
                    Ok::<(), AgentSessionError>(())
                })
                .await;
                if let Ok(result) = result {
                    result?;
                }
            }
        } else {
            self.observe();
            if self.items.is_none() {
                let _ = tokio::time::timeout(Duration::from_millis(1500), self.receiver.changed())
                    .await;
                self.items.clone_from(&self.receiver.borrow_and_update());
            }
        }
        Ok(self.items.clone())
    }
}

/// Publish only this session's standard ACP command update, preserving native ordering and hints.
/// Returns false for other frames, and failed for malformed or cross-session catalogs.
pub(super) fn publish(
    sender: &watch::Sender<Option<Vec<Value>>>,
    id: &str,
    frame: &Value,
) -> Result<bool, AgentSessionError> {
    let update = &frame["params"]["update"];
    if frame["method"] != "session/update" || update["sessionUpdate"] != "available_commands_update"
    {
        return Ok(false);
    }
    if frame["params"]["sessionId"] != id {
        return Err(AgentSessionError::Failed);
    }
    let commands = update["availableCommands"]
        .as_array()
        .filter(|items| items.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?;
    let items = commands.iter().map(|command| {
        let name = super::config::text(command, "name")?;
        if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
            return Err(AgentSessionError::Failed);
        }
        Ok(json!({"name":name,"description":command["description"].as_str().unwrap_or_default(),
            "argumentHint":command["input"]["hint"].as_str().unwrap_or_default(), "kind":"command"}))
    }).collect::<Result<Vec<_>, _>>()?;
    sender.send_replace(Some(items));
    Ok(true)
}

/// Discover draft commands using an owned, deletable native ACP session, without model inference.
/// Refuses runtimes lacking deletion and always cleans up the session it creates.
pub(super) async fn discover(
    client: &OpenCodeClient,
    spec: &AgentSessionSpec,
) -> Result<Vec<Value>, AgentSessionError> {
    super::config::validate_spec(spec)?;
    let (transport, capabilities) = launcher::spawn(client, &spec.cwd, &spec.config).await?;
    if !capabilities["sessionCapabilities"]["delete"].is_object() {
        let mut transport = transport;
        transport.close().await?;
        return Err(AgentSessionError::Unavailable);
    }
    let mut temporary = Temporary::new(client, &spec.cwd, transport);
    let result = async {
        let transport = temporary
            .transport
            .as_mut()
            .ok_or(AgentSessionError::Failed)?;
        let created = transport
            .startup_request(
                "session/new",
                json!({"cwd":spec.cwd,"mcpServers":[]}),
                client.startup_timeout,
            )
            .await?;
        let id = super::config::text(&created, "sessionId")?.to_owned();
        temporary.id = Some(id.clone());
        Catalog::new()
            .refresh(&id, Some((transport, &mut VecDeque::new())))
            .await?
            .ok_or(AgentSessionError::Unavailable)
    }
    .await;
    temporary.close().await?;
    result
}

#[cfg(test)]
mod tests;
