use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::{Value, json};

use super::{Links, MAX_CHILDREN, descriptor, valid_id};
use crate::{
    local::{images::ImageStore, opencode::streaming::Stream},
    ports::{
        agent_session::{AgentSessionError, AgentTurnEvent},
        controls::{NativeSubagent, SubagentEvent},
    },
};

#[derive(Debug)]
struct Child {
    descriptor: NativeSubagent,
    stream: Stream,
    depth: u64,
    terminal: bool,
    observed: bool,
}

/// One root's verified descendants; state survives foreground turns and idle child updates.
#[derive(Debug)]
pub(in crate::local::opencode) struct Live {
    root: String,
    cwd: String,
    images: ImageStore,
    children: BTreeMap<String, Child>,
    tasks: Links,
    pending: VecDeque<Value>,
    pending_bytes: usize,
}

impl Live {
    /// Create a root-scoped tracker without admitting descendants from another root.
    pub(in crate::local::opencode) fn new(root: String, cwd: String, images: ImageStore) -> Self {
        Self {
            root,
            cwd,
            images,
            children: BTreeMap::new(),
            tasks: Links::default(),
            pending: VecDeque::new(),
            pending_bytes: 0,
        }
    }

    /// Return verified descriptors for the existing host subagent list.
    pub(in crate::local::opencode) fn list(&self) -> Vec<NativeSubagent> {
        // Restored task metadata is historical. Only current observations may override the
        // host's persisted child lifecycle; include ancestors to preserve nested ownership.
        let mut visible = BTreeSet::new();
        for child in self.children.values().filter(|child| child.observed) {
            let mut id = child.descriptor.id.as_str();
            while let Some(child) = self.children.get(id) {
                if !visible.insert(id) {
                    break;
                }
                id = &child.descriptor.parent_id;
            }
        }
        self.children
            .values()
            .filter(|child| visible.contains(child.descriptor.id.as_str()))
            .map(|child| child.descriptor.clone())
            .collect()
    }

    /// Restore descriptors from native task history in parent-before-child order.
    pub(in crate::local::opencode) fn restore(
        &mut self,
        children: Vec<NativeSubagent>,
    ) -> Result<(), AgentSessionError> {
        for child in children {
            let depth = self
                .depth(&child.parent_id)
                .ok_or(AgentSessionError::Failed)?;
            if (self.children.len() >= MAX_CHILDREN && !self.children.contains_key(&child.id))
                || depth > 32
            {
                return Err(AgentSessionError::Failed);
            }
            self.children
                .entry(child.id.clone())
                .or_insert_with(|| Child {
                    descriptor: child,
                    stream: Stream::new(self.images.clone()),
                    depth,
                    terminal: false,
                    observed: false,
                });
        }
        Ok(())
    }

    fn depth(&self, parent: &str) -> Option<u64> {
        if parent == self.root {
            Some(1)
        } else {
            self.children.get(parent).map(|child| child.depth + 1)
        }
    }

    /// The owned ACP process has closed; its observed unfinished children cannot keep running.
    pub(in crate::local::opencode) fn stopped(&mut self) {
        for child in self.children.values_mut().filter(|child| child.observed) {
            if child.descriptor.descriptor["status"] == "running" {
                child.descriptor.descriptor["status"] = json!("canceled");
                child.descriptor.descriptor["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
                child.terminal = true;
            }
        }
    }

    /// Route negotiated child notifications and legacy tagged ACP updates away from the root.
    /// The boolean indicates that the frame belongs exclusively to a child.
    pub(in crate::local::opencode) fn consume(
        &mut self,
        message: &Value,
    ) -> Result<(bool, Vec<AgentTurnEvent>), AgentSessionError> {
        let params = &message["params"];
        let mut events = Vec::new();
        let frame = if message["method"] == "opencode/session/child_update" {
            if params["rootSessionId"] != self.root {
                return Ok((true, events));
            }
            Some(params.clone())
        } else if message["method"] == "session/update" && params["sessionId"] == self.root {
            let update = &params["update"];
            if let Some(meta) = update.pointer("/_meta/opencode~1child-session") {
                Some(
                    json!({"childSessionId":meta["id"],"parentSessionId":meta["parentID"],
                    "depth":meta["depth"],"title":meta["title"],"type":"update","update":update}),
                )
            } else {
                if let Some(link) = self.tasks.update(update)? {
                    self.task(&link, &self.root.clone(), &mut events)?;
                }
                None
            }
        } else {
            None
        };
        let handled = frame.is_some();
        if let Some(frame) = frame {
            if !self.frame(&frame, &mut events)? {
                let size = serde_json::to_vec(&frame)
                    .map_err(|_| AgentSessionError::Failed)?
                    .len();
                if self.pending.len() >= 256 || self.pending_bytes.saturating_add(size) > 512 * 1024
                {
                    return Err(AgentSessionError::Failed);
                }
                self.pending_bytes += size;
                self.pending.push_back(frame);
            }
            loop {
                let before = self.pending.len();
                for _ in 0..before {
                    let pending = self.pending.pop_front().ok_or(AgentSessionError::Failed)?;
                    if !self.frame(&pending, &mut events)? {
                        self.pending.push_back(pending);
                    }
                }
                if self.pending.is_empty() {
                    self.pending_bytes = 0;
                }
                if self.pending.len() == before || self.pending.is_empty() {
                    break;
                }
            }
        }
        Ok((handled, events))
    }

    fn task(
        &mut self,
        link: &Value,
        parent: &str,
        events: &mut Vec<AgentTurnEvent>,
    ) -> Result<(), AgentSessionError> {
        let descriptor = descriptor(link, parent, &self.cwd)?;
        let depth = self.depth(parent).ok_or(AgentSessionError::Failed)?;
        if !self.children.contains_key(&descriptor.id) {
            self.restore(vec![descriptor.clone()])?;
        }
        let child = self
            .children
            .get_mut(&descriptor.id)
            .ok_or(AgentSessionError::Failed)?;
        if child.descriptor.parent_id != parent || child.depth != depth {
            return Err(AgentSessionError::Failed);
        }
        // Explicit child lifecycle facts win over a background task's completed tool call.
        child.observed = true;
        if !child.terminal {
            child.descriptor.descriptor["status"] = descriptor.descriptor["status"].clone();
            if child.descriptor.descriptor["status"] != "running" {
                child.stream.flush();
            }
        }
        child.descriptor.descriptor["toolCallId"] = descriptor.descriptor["toolCallId"].clone();
        if !descriptor.descriptor["title"].is_null() {
            child.descriptor.descriptor["title"] = descriptor.descriptor["title"].clone();
            child.descriptor.descriptor["description"] =
                descriptor.descriptor["description"].clone();
        }
        if !descriptor.descriptor["subtitle"].is_null() {
            child.descriptor.descriptor["subtitle"] = descriptor.descriptor["subtitle"].clone();
        }
        child.descriptor.descriptor["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        events.push(AgentTurnEvent::Subagent(SubagentEvent::Upsert(
            child.descriptor.clone(),
        )));
        drain(child, events);
        Ok(())
    }

    fn frame(
        &mut self,
        frame: &Value,
        events: &mut Vec<AgentTurnEvent>,
    ) -> Result<bool, AgentSessionError> {
        let id = super::super::config::text(frame, "childSessionId")?;
        let parent = super::super::config::text(frame, "parentSessionId")?;
        valid_id(id)?;
        valid_id(parent)?;
        if id == self.root
            || id == parent
            || !frame["depth"]
                .as_u64()
                .is_some_and(|depth| (1..=32).contains(&depth))
        {
            return Err(AgentSessionError::Failed);
        }
        let Some(depth) = self.depth(parent) else {
            return Ok(false);
        };
        if frame["depth"].as_u64() != Some(depth) || depth > 32 {
            return Err(AgentSessionError::Failed);
        }
        if !self.children.contains_key(id) {
            let link = json!({"id":id,"parentID":parent,"title":frame["title"],"status":"running"});
            self.restore(vec![descriptor(&link, parent, &self.cwd)?])?;
        }
        let child = self.children.get_mut(id).ok_or(AgentSessionError::Failed)?;
        if child.descriptor.parent_id != parent {
            return Err(AgentSessionError::Failed);
        }
        let mut link = None;
        child.observed = true;
        match frame["type"].as_str() {
            Some("status") => {
                let status = match frame["status"].as_str() {
                    Some("created" | "running") => "running",
                    Some("completed") => "completed",
                    Some("failed") => "failed",
                    Some("interrupted") => "canceled",
                    _ => return Err(AgentSessionError::Failed),
                };
                child.descriptor.descriptor["status"] = json!(status);
                child.terminal = status != "running";
                if status != "running" {
                    child.stream.flush();
                }
            }
            Some("update") => {
                let mut update = frame["update"].clone();
                // Legacy ACP prefixes IDs/titles; restore native identities for history parity.
                if let Some(call) = update["toolCallId"]
                    .as_str()
                    .and_then(|call| call.strip_prefix(&format!("{id}:")))
                {
                    update["toolCallId"] = json!(call);
                }
                if let Some(title) = frame["title"].as_str()
                    && let Some(value) = update["title"]
                        .as_str()
                        .and_then(|value| value.strip_prefix(&format!("{title}: ")))
                {
                    update["title"] = json!(value);
                }
                link = child.stream.links.update(&update)?;
                child.stream.update(&update)?;
            }
            _ => return Err(AgentSessionError::Failed),
        }
        child.descriptor.descriptor["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
        events.push(AgentTurnEvent::Subagent(SubagentEvent::Upsert(
            child.descriptor.clone(),
        )));
        drain(child, events);
        if let Some(link) = link {
            self.task(&link, id, events)?;
        }
        Ok(true)
    }
}

fn drain(child: &mut Child, events: &mut Vec<AgentTurnEvent>) {
    for event in child.stream.events.drain(..) {
        let event = match event {
            AgentTurnEvent::Progress { observation, entry } => SubagentEvent::Progress {
                id: child.descriptor.id.clone(),
                observation,
                entry,
            },
            AgentTurnEvent::Timeline(entry) => SubagentEvent::Timeline {
                id: child.descriptor.id.clone(),
                entry,
            },
            _ => continue,
        };
        events.push(AgentTurnEvent::Subagent(event));
    }
}
