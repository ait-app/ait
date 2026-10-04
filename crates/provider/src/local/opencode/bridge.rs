//! Bounded asynchronous bridges for the server's permission and progress events.
use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};

use super::{
    failure,
    http::Version,
    types::{
        ApprovalRequest, ApprovalSink, ApprovalTarget, Decision, Fault, ProgressEvent,
        ProgressSink, ProtocolError,
    },
};
use crate::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};

struct Permission {
    payload: Value,
    answer: Option<oneshot::Sender<Decision>>,
}

pub(super) struct Bridge {
    pending: Mutex<BTreeMap<String, Permission>>,
    events: mpsc::Sender<AgentTurnEvent>,
    version: Version,
    turn: String,
    client_message_id: Option<String>,
}

impl Bridge {
    pub(super) fn new(
        events: mpsc::Sender<AgentTurnEvent>,
        version: Version,
        turn: String,
        client_message_id: Option<String>,
    ) -> Self {
        Self {
            pending: Mutex::default(),
            events,
            version,
            turn,
            client_message_id,
        }
    }

    pub(super) fn pending(&self) -> Vec<Value> {
        self.pending
            .lock()
            .map(|pending| {
                pending
                    .values()
                    .filter(|value| value.answer.is_some())
                    .map(|value| value.payload.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn respond(&self, id: &str, response: &Value) -> Result<(), AgentSessionError> {
        let fields = response.as_object().ok_or(AgentSessionError::Rejected)?;
        let action = response["behavior"]
            .as_str()
            .filter(|action| matches!(*action, "allow" | "deny"))
            .ok_or(AgentSessionError::Rejected)?;
        if fields.keys().any(|key| {
            !(matches!(key.as_str(), "behavior" | "selectedActionId")
                || action == "deny" && key == "message")
        }) || response
            .get("message")
            .is_some_and(|value| value.as_str().is_none_or(|message| message.len() > 4096))
            || response
                .get("selectedActionId")
                .is_some_and(|id| id.as_str() != Some(action))
        {
            return Err(AgentSessionError::Rejected);
        }
        let mut pending = self.pending.lock().map_err(|_| AgentSessionError::Failed)?;
        let answer = pending
            .get_mut(id)
            .and_then(|value| value.answer.take())
            .ok_or(AgentSessionError::Rejected)?;
        answer
            .send(if action == "allow" {
                Decision::Approved
            } else {
                Decision::Denied
            })
            .map_err(|_| AgentSessionError::Rejected)
    }

    pub(super) async fn clear(&self) {
        let ids = self
            .pending
            .lock()
            .map(|mut pending| {
                std::mem::take(&mut *pending)
                    .into_keys()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in ids {
            let _ = self
                .events
                .send(AgentTurnEvent::PermissionResolved(id))
                .await;
        }
    }
}

#[async_trait]
impl ApprovalSink for Bridge {
    async fn decide(&self, request: ApprovalRequest) -> Result<Decision, ProtocolError> {
        let (name, input) = match request.target {
            ApprovalTarget::Command { command, cwd } => {
                ("Shell", json!({"command":command,"cwd":cwd}))
            }
            ApprovalTarget::Files { paths } => ("Edit", json!({"paths":paths})),
        };
        let payload = json!({"id":request.id,"provider":"opencode","kind":"tool","name":name,"input":input,"actions":[
            {"id":"allow","label":"Allow once","behavior":"allow","variant":"primary"},
            {"id":"deny","label":"Deny","behavior":"deny","variant":"secondary"}]});
        let (answer, receiver) = oneshot::channel();
        {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| failure(Fault::ProviderFailed, "permission lock"))?;
            if pending.len() >= 16 || pending.contains_key(&request.id) {
                return Err(failure(Fault::ProviderFailed, "duplicate permission"));
            }
            pending.insert(
                request.id,
                Permission {
                    payload: payload.clone(),
                    answer: Some(answer),
                },
            );
        }
        self.events
            .send(AgentTurnEvent::PermissionRequested(payload))
            .await
            .map_err(|_| failure(Fault::RunCancelled, "session closed"))?;
        Ok(receiver.await.unwrap_or(Decision::Cancelled))
    }

    async fn expire(&self, request: &ApprovalRequest) -> Result<(), ProtocolError> {
        self.resolved(&request.id).await
    }

    async fn resolved(&self, id: &str) -> Result<(), ProtocolError> {
        let removed = self
            .pending
            .lock()
            .map_err(|_| failure(Fault::ProviderFailed, "permission lock"))?
            .remove(id)
            .is_some();
        if removed {
            let _ = self
                .events
                .send(AgentTurnEvent::PermissionResolved(id.to_owned()))
                .await;
        }
        Ok(())
    }
}

#[async_trait]
impl ProgressSink for Bridge {
    async fn report(&self, event: ProgressEvent) {
        let (id, delta) = match event {
            ProgressEvent::Timeline(mut entry) => {
                if entry.item["type"] == "user_message"
                    && let Some(client) = &self.client_message_id
                {
                    entry.item["clientMessageId"] = json!(client);
                }
                let _ = self.events.send(AgentTurnEvent::Timeline(*entry)).await;
                return;
            }
            ProgressEvent::TextDelta { id, delta } => (id, delta),
        };
        let key = if self.version == Version::V1 {
            id
        } else {
            format!("{id}:0")
        };
        let entry = NativeItem {
            key: super::projection::key(&key),
            turn_id: Some(self.turn.clone()),
            timestamp: chrono::Utc::now().to_rfc3339(),
            item: json!({"type":"assistant_message","messageId":key,"text":delta}),
        };
        let _ = self
            .events
            .send(AgentTurnEvent::Progress {
                observation: uuid::Uuid::new_v4().to_string(),
                entry,
            })
            .await;
    }
}
