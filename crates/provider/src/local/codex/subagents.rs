//! Only native spawn provenance can enroll a child stream in this session.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{discovery, streaming};
use crate::local::images::ImageStore;
use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};
use crate::ports::controls::{NativeSubagent, SubagentEvent};

#[derive(Debug)]
struct Child {
    descriptor: NativeSubagent,
    stream: streaming::Stream,
    terminal: super::terminal::Terminal,
    turn: Option<String>,
    finished: BTreeSet<String>,
}

#[derive(Debug, Default)]
pub(super) struct Live {
    children: BTreeMap<String, Child>,
    spawning: BTreeSet<String>,
    buffered: std::collections::VecDeque<(String, Value)>,
    buffered_bytes: usize,
}

impl Live {
    fn activity(
        &mut self,
        params: &Value,
        context: (&str, &str),
        events: &mut Vec<AgentTurnEvent>,
    ) -> Result<(), AgentSessionError> {
        let item = &params["item"];
        let sender = super::native_sessions::text(params, "threadId")?;
        let receiver = super::native_sessions::text(item, "agentThreadId")?;
        let id = super::native_sessions::text(item, "id")?;
        let kind = item["kind"].as_str().ok_or(AgentSessionError::Failed)?;
        if receiver == sender
            || !matches!(kind, "started" | "interacted" | "interrupted" | "completed")
        {
            return Err(AgentSessionError::Failed);
        }
        // Communication can target an ancestor or peer; only a start proves child ancestry.
        if kind != "started" {
            if let Some(child) = self.children.get_mut(receiver) {
                match kind {
                    "interrupted" => set_status(child, "canceled", events),
                    // The child's own terminal event retains a more specific failure/cancel state.
                    "completed" if child.descriptor.descriptor["status"] == "running" => {
                        set_status(child, "completed", events);
                    }
                    _ => {}
                }
            }
            return Ok(());
        }
        if receiver == context.0 {
            return Err(AgentSessionError::Failed);
        }
        let detail = crate::local::tool_detail::codex(item);
        if detail["type"] != "sub_agent" {
            return Err(AgentSessionError::Failed);
        }
        self.collab(&json!({"id":id,"tool":"spawnAgent","senderThreadId":sender,
            "receiverThreadIds":[receiver],"model":item["model"],"reasoningEffort":item["reasoningEffort"],
            "prompt":detail["description"]}), sender, context.1, events)?;
        let child = self
            .children
            .get_mut(receiver)
            .ok_or(AgentSessionError::Failed)?;
        if child.descriptor.descriptor["title"].is_null() {
            child.descriptor.descriptor["title"] = detail["subAgentType"].clone();
        }
        if child.descriptor.descriptor["toolCallId"].is_null() {
            child.descriptor.descriptor["toolCallId"] = json!(id);
        }
        events.push(upsert(child));
        Ok(())
    }

    fn buffer(&mut self, method: &str, params: &Value) -> Result<(), AgentSessionError> {
        let size = serde_json::to_vec(params)
            .map_err(|_| AgentSessionError::Failed)?
            .len();
        if self.buffered.len() >= 256 || self.buffered_bytes.saturating_add(size) > 512 * 1024 {
            return Err(AgentSessionError::Failed);
        }
        self.buffered_bytes += size;
        self.buffered.push_back((method.to_owned(), params.clone()));
        Ok(())
    }

    fn announce(
        &mut self,
        thread: &Value,
        context: (&str, &str),
        events: &mut Vec<AgentTurnEvent>,
    ) -> Result<(), AgentSessionError> {
        let Some(parent) = super::native_sessions::parent(thread)? else {
            return Ok(());
        };
        if parent != context.0 && !self.children.contains_key(&parent) {
            return Ok(());
        }
        let facts = super::native_sessions::descriptor(thread)?;
        if let Some(child) = self.children.get_mut(&facts.provider_handle_id) {
            if child.descriptor.parent_id != parent {
                return Err(AgentSessionError::Failed);
            }
            child.descriptor.cwd.clone_from(&facts.cwd);
            child.descriptor.descriptor["cwd"] = json!(facts.cwd);
            events.push(upsert(child));
        } else {
            self.collab(&json!({"id":thread["source"]["subAgent"]["thread_spawn"]["tool_call_id"],
                "tool":"spawnAgent","senderThreadId":parent,"receiverThreadIds":[facts.provider_handle_id],
                "model":thread["model"],"prompt":thread["preview"]}), &parent,&facts.cwd,events)?;
        }
        Ok(())
    }

    pub(super) fn restore(
        children: Vec<NativeSubagent>,
        root: &str,
    ) -> Result<Self, AgentSessionError> {
        let mut live = Self::default();
        let mut pending = children;
        loop {
            let before = pending.len();
            let mut remaining = Vec::new();
            for descriptor in pending {
                if descriptor.parent_id != root
                    && !live.children.contains_key(&descriptor.parent_id)
                {
                    remaining.push(descriptor);
                    continue;
                }
                if descriptor.id == root || live.children.len() >= 256 {
                    return Err(AgentSessionError::Failed);
                }
                live.children.insert(
                    descriptor.id.clone(),
                    Child {
                        descriptor,
                        stream: streaming::Stream::default(),
                        terminal: super::terminal::Terminal::default(),
                        turn: None,
                        finished: BTreeSet::new(),
                    },
                );
            }
            if remaining.len() == before || remaining.is_empty() {
                break;
            }
            pending = remaining;
        }
        Ok(live)
    }

    pub(super) fn stopped(&mut self) -> Vec<AgentTurnEvent> {
        let mut events = Vec::new();
        for child in self.children.values_mut() {
            if child.descriptor.descriptor["status"] == "running" {
                set_status(child, "canceled", &mut events);
            }
        }
        events
    }
    pub(super) fn children(&self) -> Vec<NativeSubagent> {
        self.children
            .values()
            .map(|child| child.descriptor.clone())
            .collect()
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.children.contains_key(id)
    }

    pub(super) fn observe(
        &mut self,
        method: &str,
        params: &Value,
        context: (&str, &str),
        images: &ImageStore,
    ) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
        let (root, cwd) = context;
        let mut events = Vec::new();
        if method == "thread/started" {
            self.announce(&params["thread"], context, &mut events)?;
            return Ok(events);
        }
        let Some(thread) = params["threadId"].as_str() else {
            return Ok(events);
        };
        if thread != root && !self.children.contains_key(thread) {
            if !self.spawning.is_empty() {
                self.buffer(method, params)?;
            }
            return Ok(events);
        }
        if matches!(method, "item/started" | "item/completed")
            && params["item"]["type"] == "subAgentActivity"
        {
            self.activity(params, context, &mut events)?;
        }
        if matches!(method, "item/started" | "item/completed")
            && params["item"]["type"] == "collabAgentToolCall"
        {
            let item = &params["item"];
            if item["tool"] == "spawnAgent"
                && let Some(id) = item["id"].as_str()
            {
                if method == "item/started" {
                    self.spawning.insert(id.to_owned());
                } else {
                    self.spawning.remove(id);
                }
            }
            self.collab(item, thread, cwd, &mut events)?;
            let buffered = std::mem::take(&mut self.buffered);
            self.buffered_bytes = 0;
            for (method, params) in buffered {
                events.extend(self.observe(&method, &params, context, images)?);
            }
        }
        if thread == root {
            return Ok(events);
        }
        let child = self
            .children
            .get_mut(thread)
            .ok_or(AgentSessionError::Failed)?;
        events.extend(observe_child(child, method, params, images)?);
        Ok(events)
    }

    fn collab(
        &mut self,
        item: &Value,
        sender: &str,
        cwd: &str,
        events: &mut Vec<AgentTurnEvent>,
    ) -> Result<(), AgentSessionError> {
        if item["senderThreadId"] != sender {
            return Err(AgentSessionError::Failed);
        }
        let receivers = item["receiverThreadIds"]
            .as_array()
            .filter(|receivers| receivers.len() <= 128)
            .ok_or(AgentSessionError::Failed)?;
        for receiver in receivers {
            let id = receiver
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 256 && *id != sender)
                .ok_or(AgentSessionError::Failed)?;
            if !self.children.contains_key(id) && item["tool"] == "spawnAgent" {
                if self.children.len() >= 256 {
                    return Err(AgentSessionError::Failed);
                }
                let now = discovery::timestamp();
                let subtitle = [item["model"].as_str(), item["reasoningEffort"].as_str()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" · ");
                let child = Child {
                    descriptor: NativeSubagent {
                        persistence: None,
                        id: id.to_owned(),
                        parent_id: sender.to_owned(),
                        cwd: cwd.to_owned(),
                        descriptor: json!({"id":id,"provider":"codex","title":null,"description":item["prompt"],"status":"running",
                        "createdAt":now,"updatedAt":now,"toolCallId":item["id"],"cwd":cwd,"subtitle":subtitle}),
                    },
                    stream: streaming::Stream::default(),
                    terminal: super::terminal::Terminal::default(),
                    turn: None,
                    finished: BTreeSet::new(),
                };
                events.push(upsert(&child));
                self.children.insert(id.to_owned(), child);
            }
            let Some(child) = self.children.get_mut(id) else {
                continue;
            };
            let status = match item["agentsStates"][id]["status"].as_str() {
                Some("pendingInit" | "running") => "running",
                Some("completed") => "completed",
                Some("interrupted" | "shutdown") => "canceled",
                Some("errored" | "notFound") => "failed",
                _ => continue,
            };
            set_status(child, status, events);
        }
        Ok(())
    }
}

fn observe_child(
    child: &mut Child,
    method: &str,
    params: &Value,
    images: &ImageStore,
) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
    let mut events = Vec::new();
    let thread = params["threadId"]
        .as_str()
        .ok_or(AgentSessionError::Failed)?;
    let compaction_turn = if method == "thread/compacted" {
        child.turn.clone()
    } else {
        None
    };
    let Some(turn) = params["turnId"]
        .as_str()
        .or_else(|| params["turn"]["id"].as_str())
        .or(compaction_turn.as_deref())
    else {
        return Ok(events);
    };
    if child.finished.contains(turn) {
        return Ok(events);
    }
    if child.turn.as_deref() != Some(turn) {
        if child.turn.is_some() || child.finished.len() >= 256 {
            return Err(AgentSessionError::Failed);
        }
        child.turn = Some(turn.to_owned());
        child.stream = streaming::Stream::default();
        set_status(child, "running", &mut events);
    }
    if method == "turn/completed" {
        child.stream.finish_compactions(turn)?;
    }
    let terminal = if method == "thread/compacted" {
        child.stream.compacted(Some(turn))?
    } else {
        child.terminal.observe(method, params)?
    };
    let progress = terminal.or(child.stream.progress(method, params)?);
    for event in progress.into_iter().chain(child.stream.events.drain(..)) {
        if let AgentTurnEvent::Progress { observation, entry } = event {
            events.push(AgentTurnEvent::Subagent(SubagentEvent::Progress {
                id: thread.to_owned(),
                observation,
                entry,
            }));
        } else if let AgentTurnEvent::Timeline(entry) = event {
            events.push(AgentTurnEvent::Subagent(SubagentEvent::Timeline {
                id: thread.to_owned(),
                entry,
            }));
        }
    }
    if method == "item/completed" && child.stream.complete(&params["item"])? {
        for entry in
            discovery::timeline_items(&params["item"], turn, &discovery::timestamp(), images)?
        {
            events.push(AgentTurnEvent::Subagent(SubagentEvent::Timeline {
                id: thread.to_owned(),
                entry,
            }));
        }
    }
    if method == "turn/completed" {
        set_status(
            child,
            match params["turn"]["status"].as_str() {
                Some("completed") => "completed",
                Some("interrupted") => "canceled",
                _ => "failed",
            },
            &mut events,
        );
        child.turn = None;
        child.finished.insert(turn.to_owned());
    }
    Ok(events)
}

fn upsert(child: &Child) -> AgentTurnEvent {
    AgentTurnEvent::Subagent(SubagentEvent::Upsert(child.descriptor.clone()))
}

fn set_status(child: &mut Child, status: &str, events: &mut Vec<AgentTurnEvent>) {
    if child.descriptor.descriptor["status"] == status {
        return;
    }
    child.descriptor.descriptor["status"] = json!(status);
    child.descriptor.descriptor["updatedAt"] = json!(discovery::timestamp());
    events.push(upsert(child));
}

#[cfg(test)]
mod tests;
