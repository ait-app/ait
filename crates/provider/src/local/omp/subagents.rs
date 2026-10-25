use std::collections::BTreeMap;

use chrono::Utc;
use serde_json::{Value, json};

use super::{config, stream::Stream};
use crate::local::images::ImageStore;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::ports::controls::{NativeSubagent, SubagentEvent};

#[derive(Debug)]
struct Child {
    descriptor: NativeSubagent,
    stream: Stream,
}

#[derive(Debug, Default)]
pub(super) struct Children(BTreeMap<String, Child>);

impl Children {
    pub(super) fn list(&self) -> Vec<NativeSubagent> {
        self.0
            .values()
            .map(|child| child.descriptor.clone())
            .collect()
    }

    pub(super) fn consume(
        &mut self,
        frame: &Value,
        parent: &str,
        cwd: &str,
        images: &ImageStore,
    ) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
        let payload = &frame["payload"];
        if frame["type"] == "subagent_event" {
            let id = config::text(payload, "id")?;
            let Some(child) = self.0.get_mut(id) else {
                return Ok(Vec::new());
            };
            child
                .stream
                .consume(&payload["event"])
                .map_err(|_| AgentSessionError::Failed)?;
            return Ok(child
                .stream
                .events
                .drain(..)
                .filter_map(|event| match event {
                    AgentTurnEvent::Progress { observation, entry } => {
                        Some(AgentTurnEvent::Subagent(SubagentEvent::Progress {
                            id: id.to_owned(),
                            observation,
                            entry,
                        }))
                    }
                    AgentTurnEvent::Timeline(entry) => {
                        Some(AgentTurnEvent::Subagent(SubagentEvent::Timeline {
                            id: id.to_owned(),
                            entry,
                        }))
                    }
                    _ => None,
                })
                .collect());
        }
        let progress = frame["type"] == "subagent_progress";
        let source = if progress {
            &payload["progress"]
        } else {
            payload
        };
        let id = config::text(source, "id")?;
        if id == parent || (self.0.len() >= 256 && !self.0.contains_key(id)) {
            return Err(AgentSessionError::Failed);
        }
        let status = match source["status"].as_str() {
            Some("started" | "pending" | "running") => "running",
            Some("completed") => "completed",
            Some("failed") => "failed",
            Some("aborted") => "canceled",
            _ => return Err(AgentSessionError::Failed),
        };
        let now = Utc::now().to_rfc3339();
        let child = self.0.entry(id.to_owned()).or_insert_with(|| Child {
            descriptor: NativeSubagent {
                persistence: None,
                id: id.to_owned(),
                parent_id: parent.to_owned(),
                cwd: cwd.to_owned(),
                descriptor: json!({"id":id,"provider":"omp","title":payload["agent"],"cwd":cwd,
                    "description":payload["description"],"createdAt":now,"updatedAt":now,
                    "toolCallId":payload["parentToolCallId"],"status":status}),
            },
            stream: Stream::new(id.to_owned(), images.clone()),
        });
        child.descriptor.descriptor["status"] = json!(status);
        child.descriptor.descriptor["updatedAt"] = json!(now);
        if progress {
            child.descriptor.descriptor["description"] = source["task"].clone();
            child.descriptor.descriptor["subtitle"] = source["resolvedModel"].clone();
        }
        Ok(vec![AgentTurnEvent::Subagent(SubagentEvent::Upsert(
            child.descriptor.clone(),
        ))])
    }
}

#[cfg(test)]
mod tests;
