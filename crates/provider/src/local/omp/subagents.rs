use std::collections::BTreeMap;
use std::path::PathBuf;

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
pub(super) struct Children {
    children: BTreeMap<String, Child>,
    blobs: Option<PathBuf>,
}

impl Children {
    /// Use the parent session's native blob directory for child image projection.
    pub(super) fn with_blobs(mut self, directory: PathBuf) -> Self {
        self.blobs = Some(directory);
        self
    }

    pub(super) fn list(&self) -> Vec<NativeSubagent> {
        self.children
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
            let Some(child) = self.children.get_mut(id) else {
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
        if id == parent || (self.children.len() >= 256 && !self.children.contains_key(id)) {
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
        let blobs = &self.blobs;
        let child = self.children.entry(id.to_owned()).or_insert_with(|| Child {
            descriptor: NativeSubagent {
                persistence: None,
                id: id.to_owned(),
                parent_id: parent.to_owned(),
                cwd: cwd.to_owned(),
                descriptor: json!({"id":id,"provider":"omp","title":payload["agent"],"cwd":cwd,
                    "description":payload["description"],"createdAt":now,"updatedAt":now,
                    "toolCallId":payload["parentToolCallId"],"status":status}),
            },
            stream: {
                let stream = Stream::new(id.to_owned(), images.clone());
                match blobs {
                    Some(directory) => stream.with_blobs(directory.clone()),
                    None => stream,
                }
            },
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
