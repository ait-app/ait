//! ACP remains the writer and transcript owner. Only missing 1.18 metadata uses its owned server.
mod http;
mod projection;

use std::{collections::BTreeSet, time::Duration};

use serde_json::{Value, json};
use tokio::{process::Command, sync::mpsc};
use tokio_util::task::AbortOnDropHandle;

use crate::{ports::agent_session::AgentSessionError, protocol::timeline::NativeItem};

/// Ephemeral address and authentication for the native server inside one ACP child.
#[derive(Debug)]
pub(super) struct Source {
    prepared: Option<http::Prepared>,
    client: http::Client,
}

impl Source {
    /// Enable the read-only supplement only for the verified legacy ACP profile.
    pub(super) fn prepare(
        version: &str,
        deadline: Duration,
    ) -> Result<Option<Self>, AgentSessionError> {
        if !version.starts_with("1.18.") {
            return Ok(None);
        }
        let prepared = http::Prepared::new(deadline)?;
        Ok(Some(Self {
            client: prepared.client.clone(),
            prepared: Some(prepared),
        }))
    }

    /// Configure the owned child's loopback server; no existing native server is attached.
    pub(super) fn configure(&self, command: &mut Command) -> Result<(), AgentSessionError> {
        self.prepared
            .as_ref()
            .ok_or(AgentSessionError::Failed)?
            .configure(command)
    }

    /// Release the reserved address immediately before the child binds it.
    pub(super) fn release(&mut self) {
        self.prepared = None;
    }

    /// Subscribe before reading a snapshot so updates during restoration cannot be lost.
    pub(super) async fn bind(self, id: &str, cwd: &str) -> Result<Observation, AgentSessionError> {
        if id.is_empty()
            || id.len() > 1024
            || id.chars().any(char::is_control)
            || id.contains(['/', '\\', '?', '#', '%'])
        {
            return Err(AgentSessionError::Rejected);
        }
        let events = self.client.events(cwd).await?;
        let (sender, receiver) = mpsc::channel(128);
        let client = self.client.clone();
        let cwd_owned = cwd.to_owned();
        let session = id.to_owned();
        let task = AbortOnDropHandle::new(tokio::spawn(async move {
            let mut events = events;
            loop {
                if let Ok(event) = events.next().await {
                    if relevant(&event, &session) && sender.send(event).await.is_err() {
                        break;
                    }
                } else {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if let Ok(next) = client.events(&cwd_owned).await {
                            events = next;
                            break;
                        }
                    }
                    if sender
                        .send(json!({"type":"ait.observation.resync"}))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }));
        let mut observation = Observation {
            client: self.client,
            id: id.to_owned(),
            cwd: cwd.to_owned(),
            projection: projection::Projection::default(),
            receiver,
            _task: task,
            initial: Vec::new(),
            resync: false,
        };
        observation.refresh(true).await?;
        observation.initial = observation.projection.usage.clone().into_iter().collect();
        Ok(observation)
    }
}

/// Session-scoped native facts; dropping it cancels the subscription and releases all HTTP resources.
#[derive(Debug)]
pub(super) struct Observation {
    client: http::Client,
    id: String,
    cwd: String,
    projection: projection::Projection,
    receiver: mpsc::Receiver<Value>,
    _task: AbortOnDropHandle<()>,
    pub(super) initial: Vec<Value>,
    resync: bool,
}

impl Observation {
    /// Wait for metadata without blocking ACP tools, callbacks or cancellation.
    pub(super) async fn next(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        loop {
            if self.resync {
                let updates = self.refresh(true).await?;
                self.resync = false;
                return Ok(updates);
            }
            let Some(event) = self.receiver.recv().await else {
                return std::future::pending().await;
            };
            if event["type"] == "ait.observation.resync" {
                self.resync = true;
                continue;
            }
            return self.projection.event(&self.id, &event);
        }
    }

    /// Refresh metadata from bounded native pages. History text and tools still come from ACP.
    pub(super) async fn refresh(&mut self, full: bool) -> Result<Vec<Value>, AgentSessionError> {
        tokio::time::timeout(self.client.deadline, self.refresh_inner(full))
            .await
            .map_err(|_| AgentSessionError::Failed)?
    }

    async fn refresh_inner(&mut self, full: bool) -> Result<Vec<Value>, AgentSessionError> {
        let mut projection = self.projection.clone();
        let (catalog, _): (Value, _) = self
            .client
            .json(self.client.get("/config/providers", &self.cwd))
            .await?;
        projection.catalog(&catalog)?;
        let (session, _): (Value, _) = self
            .client
            .json(self.client.get(&format!("/session/{}", self.id), &self.cwd))
            .await?;
        if session["id"] != self.id {
            return Err(AgentSessionError::Failed);
        }
        let mut updates = projection.session(&session);
        let mut cursor = None;
        let mut cursors = BTreeSet::new();
        let mut count = 0;
        loop {
            let mut request = self
                .client
                .get(&format!("/session/{}/message", self.id), &self.cwd)
                .query(&[("limit", "32")]);
            if let Some(cursor) = &cursor {
                request = request.query(&[("before", cursor)]);
            }
            let (rows, next): (Vec<projection::Row>, _) = self.client.json(request).await?;
            count += rows.len();
            if count > 65_536 || rows.iter().any(|row| row.info["sessionID"] != self.id) {
                return Err(AgentSessionError::Failed);
            }
            updates.extend(projection.snapshot(&rows)?);
            cursor = next;
            if !full || cursor.is_none() {
                break;
            }
            if !cursors.insert(cursor.clone()) {
                return Err(AgentSessionError::Failed);
            }
        }
        self.projection = projection;
        Ok(updates)
    }

    /// Keep genuine request usage when legacy ACP reports an aborted zero-token placeholder.
    pub(super) fn has_usage(&self) -> bool {
        self.projection.usage.is_some()
    }

    /// Capture the last observed assistant identity before admitting another ACP prompt.
    pub(super) fn begin(&mut self) {
        self.projection.begin();
    }

    /// Return only a new native unknown finish from this prompt, never an earlier session outcome.
    pub(super) fn unknown_finish(&self) -> bool {
        self.projection.unknown_finish()
    }

    /// Latest factual usage for restore/import; omitted fields remain unknown.
    pub(super) fn usage(&self) -> Option<crate::protocol::usage::AgentUsage> {
        let usage = self.projection.usage.as_ref()?;
        let usage = crate::protocol::usage::AgentUsage {
            context_window_used_tokens: usage["used"].as_u64(),
            context_window_max_tokens: usage["size"].as_u64(),
            total_cost_usd: usage["cost"]["amount"].as_f64(),
            ..Default::default()
        };
        usage.is_valid().then_some(usage)
    }

    /// Supplement ACP replay with stable native compaction identities, using ACP's accepted turns.
    pub(super) fn history(
        &self,
        entries: &mut Vec<NativeItem>,
        clients: &std::collections::BTreeMap<String, String>,
    ) {
        self.projection.history(entries, clients);
    }

    /// Include native compaction-only user messages in the accepted input's persisted identity map.
    pub(super) fn users(&self) -> &BTreeSet<String> {
        &self.projection.users
    }

    /// Finish pending observations only after native prompt completion has been confirmed by ACP.
    pub(super) fn finish(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        self.projection.finish()
    }
}

/// Observe only facts from this parent session; child sessions retain independent ownership.
fn relevant(event: &Value, id: &str) -> bool {
    match event["type"].as_str() {
        Some("message.updated" | "session.updated") => {
            event["properties"]["info"]["sessionID"] == id
                || event["properties"]["info"]["id"] == id
        }
        Some("message.part.updated") => {
            event["properties"]["part"]["sessionID"] == id
                && event["properties"]["part"]["type"] == "compaction"
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
