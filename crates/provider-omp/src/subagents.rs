//! Live OMP subagents (D16), from `subagent_lifecycle`, `subagent_progress` and
//! `subagent_event` frames, as Paseo's `subagent-index.ts` tracks them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::{Value, json};

use super::history::children;
use super::{PROVIDER, config, stream::Stream};
use provider_core::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use provider_core::ports::controls::{NativeSubagent, SubagentEvent};
use provider_native::images::ImageStore;

/// Children tracked per parent session; finished ones are forgotten first, oldest first.
const MAX_CHILDREN: usize = 256;

/// Paseo's `formatOmpSubagentTitle`: the agent's name, with the model it resolved to as
/// `name · model (provider)`.
pub(super) fn title(agent: Option<&str>, model: Option<&str>) -> String {
    let name = agent
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("OMP subagent");
    let Some(model) = model.map(str::trim).filter(|model| !model.is_empty()) else {
        return name.to_owned();
    };
    match model.split_once('/') {
        Some((provider, id)) if !provider.is_empty() && !id.is_empty() => {
            format!("{name} · {id} ({provider})")
        }
        _ => format!("{name} · {model}"),
    }
}

/// The parent session whose child frames are consumed.
#[derive(Debug, Clone, Copy)]
pub(super) struct Parent<'a> {
    /// Native session id.
    pub(super) id: &'a str,
    /// Native session file, which locates its children's transcripts.
    pub(super) file: Option<&'a Path>,
    pub(super) cwd: &'a str,
}

#[derive(Debug)]
struct Child {
    /// Order in which the child was first reported.
    order: u64,
    descriptor: NativeSubagent,
    agent: Option<String>,
    model: Option<String>,
    stream: Stream,
}

#[derive(Debug, Default)]
pub(super) struct Children {
    tracked: BTreeMap<String, Child>,
    blobs: Option<PathBuf>,
    /// Children reported so far, ordering them.
    reported: u64,
}

impl Children {
    /// Use the parent session's native blob directory for child image projection.
    pub(super) fn with_blobs(mut self, directory: PathBuf) -> Self {
        self.blobs = Some(directory);
        self
    }

    pub(super) fn list(&self) -> Vec<NativeSubagent> {
        self.tracked
            .values()
            .map(|child| child.descriptor.clone())
            .collect()
    }

    /// Mark every running child canceled once its parent turn ended without it, as Paseo
    /// does, and return the upserts that publish the change.
    pub(super) fn terminalize(&mut self) -> Vec<AgentTurnEvent> {
        let now = Utc::now().to_rfc3339();
        self.tracked
            .values_mut()
            .filter(|child| child.descriptor.descriptor["status"] == "running")
            .map(|child| {
                child.descriptor.descriptor["status"] = json!("canceled");
                child.descriptor.descriptor["updatedAt"] = json!(now);
                AgentTurnEvent::Subagent(SubagentEvent::Upsert(child.descriptor.clone()))
            })
            .collect()
    }

    /// Project one child frame of `parent`. A child is named by its job id qualified with
    /// the parent's id, as its history transcript is (M6).
    ///
    /// # Errors
    ///
    /// Returns failed for a frame without a valid child id. A new child beyond 256 replaces
    /// the oldest finished one; without one, its frame is dropped.
    pub(super) fn consume(
        &mut self,
        frame: &Value,
        parent: Parent<'_>,
        images: &ImageStore,
    ) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
        let payload = &frame["payload"];
        if frame["type"] == "subagent_event" {
            let job = config::text(payload, "id")?;
            return Ok(self.event(&children::child_id(parent.id, job), &payload["event"]));
        }
        let progress = frame["type"] == "subagent_progress";
        let source = if progress {
            &payload["progress"]
        } else {
            payload
        };
        let job = config::text(source, "id")?;
        if !children::valid_job(job) {
            return Err(AgentSessionError::Failed);
        }
        let id = children::child_id(parent.id, job);
        let status = match source["status"].as_str() {
            Some("started" | "pending" | "running") => "running",
            Some("completed") => "completed",
            Some("failed") => "failed",
            Some("aborted") => "canceled",
            _ => {
                tracing::debug!("Ignoring an unknown OMP child status");
                return Ok(Vec::new());
            }
        };
        let known = self.tracked.contains_key(&id);
        if !known && self.tracked.len() >= MAX_CHILDREN && !self.evict_finished() {
            // A child display must not fail the parent turn.
            tracing::debug!("Dropping an OMP child frame beyond the tracked child limit");
            return Ok(Vec::new());
        }
        let now = Utc::now().to_rfc3339();
        let blobs = &self.blobs;
        let order = self.reported;
        if !known {
            self.reported += 1;
        }
        let child = self.tracked.entry(id.clone()).or_insert_with(|| Child {
            order,
            descriptor: NativeSubagent {
                persistence: parent
                    .file
                    .map(|file| children::handle(file, parent.id, job)),
                id: id.clone(),
                parent_id: parent.id.to_owned(),
                cwd: parent.cwd.to_owned(),
                descriptor: json!({"id":id,"provider":PROVIDER,"cwd":parent.cwd,
                    "createdAt":now,"description":null,"toolCallId":null}),
            },
            agent: None,
            model: None,
            stream: {
                let stream = Stream::new(id.clone(), images.clone());
                match blobs {
                    Some(directory) => stream.with_blobs(directory.clone()),
                    None => stream,
                }
            },
        });
        // Paseo's `handleLifecycle` and `handleProgress`: later reports refine earlier ones.
        if let Some(agent) = payload["agent"].as_str().filter(|agent| !agent.is_empty()) {
            child.agent = Some(agent.to_owned());
        }
        if let Some(model) = source["resolvedModel"]
            .as_str()
            .filter(|model| !model.trim().is_empty())
        {
            child.model = Some(model.to_owned());
        }
        let description = if progress {
            source["description"]
                .as_str()
                .or_else(|| payload["assignment"].as_str())
        } else {
            payload["description"].as_str()
        };
        let descriptor = &mut child.descriptor.descriptor;
        if let Some(description) = description {
            descriptor["description"] = json!(description);
        }
        if let Some(call) = payload["parentToolCallId"].as_str() {
            descriptor["toolCallId"] = json!(call);
        }
        descriptor["title"] = json!(title(child.agent.as_deref(), child.model.as_deref()));
        descriptor["status"] = json!(status);
        descriptor["updatedAt"] = json!(now);
        Ok(vec![AgentTurnEvent::Subagent(SubagentEvent::Upsert(
            child.descriptor.clone(),
        ))])
    }

    /// Forget the oldest child that is no longer running, already published and kept by
    /// the host's timeline. Returns whether one was forgotten.
    fn evict_finished(&mut self) -> bool {
        let oldest = self
            .tracked
            .iter()
            .filter(|(_, child)| child.descriptor.descriptor["status"] != "running")
            .min_by_key(|(_, child)| child.order)
            .map(|(id, _)| id.clone());
        oldest.is_some_and(|id| self.tracked.remove(&id).is_some())
    }

    /// Project a known child's session `event` into its own timeline.
    fn event(&mut self, id: &str, event: &Value) -> Vec<AgentTurnEvent> {
        let Some(child) = self.tracked.get_mut(id) else {
            return Vec::new();
        };
        if child.stream.consume(event).is_err() {
            // A child's display projection must not fail the parent turn.
            tracing::debug!("Ignoring an OMP child event that could not be projected");
            child.stream.events.clear();
            return Vec::new();
        }
        child
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
            .collect()
    }
}

#[cfg(test)]
mod tests;
