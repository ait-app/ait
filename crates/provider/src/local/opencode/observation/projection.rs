//! Project native metadata into the same usage and compaction updates consumed from ACP.
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    ports::agent_session::AgentSessionError,
    protocol::{timeline::NativeItem, usage::AgentUsage},
};

/// Only metadata and part kinds are decoded; tool outputs and message text are discarded.
#[derive(Debug, Deserialize)]
pub(super) struct Row {
    pub(super) info: Value,
    #[serde(default)]
    pub(super) parts: Vec<Part>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Part {
    #[serde(rename = "type")]
    pub(super) kind: String,
    pub(super) auto: Option<bool>,
}

#[derive(Clone, Debug)]
struct Marker {
    id: String,
    anchor: Option<String>,
    status: String,
    trigger: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Projection {
    pub(super) limits: BTreeMap<String, u64>,
    pub(super) usage: Option<Value>,
    latest: Option<(u64, String)>,
    outcome: Option<((u64, String), Option<String>)>,
    baseline: Option<(u64, String)>,
    cost: Option<f64>,
    markers: BTreeMap<String, Marker>,
    pub(super) users: BTreeSet<String>,
    messages: BTreeMap<String, u64>,
}

impl Projection {
    /// Retain a bounded catalog of native context limits without storing provider credentials.
    pub(super) fn catalog(&mut self, value: &Value) -> Result<(), AgentSessionError> {
        self.limits.clear();
        for provider in value["providers"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?
        {
            let id = provider["id"].as_str().ok_or(AgentSessionError::Failed)?;
            for (model, facts) in provider["models"]
                .as_object()
                .ok_or(AgentSessionError::Failed)?
            {
                if let Some(size) = facts["limit"]["context"].as_u64().filter(|size| *size > 0) {
                    if self.limits.len() >= 16_384 {
                        return Err(AgentSessionError::Failed);
                    }
                    self.limits.insert(format!("{id}/{model}"), size);
                }
            }
        }
        Ok(())
    }

    /// Consume native facts for the exact parent session; subagent and unrelated traffic is ignored.
    pub(super) fn event(
        &mut self,
        id: &str,
        event: &Value,
    ) -> Result<Vec<Value>, AgentSessionError> {
        let properties = &event["properties"];
        match event["type"].as_str() {
            Some("session.updated") if properties["info"]["id"] == id => {
                Ok(self.session(&properties["info"]))
            }
            Some("message.updated") if properties["info"]["sessionID"] == id => {
                self.message(&properties["info"])
            }
            Some("message.part.updated")
                if properties["part"]["sessionID"] == id
                    && properties["part"]["type"] == "compaction" =>
            {
                let part = &properties["part"];
                self.marker(
                    part["messageID"]
                        .as_str()
                        .ok_or(AgentSessionError::Failed)?,
                    "started",
                    None,
                    part["auto"].as_bool(),
                )
            }
            _ => Ok(Vec::new()),
        }
    }

    fn message(&mut self, info: &Value) -> Result<Vec<Value>, AgentSessionError> {
        let id = info["id"].as_str().ok_or(AgentSessionError::Failed)?;
        if let Some(time) = info["time"]["created"].as_u64() {
            if self.messages.len() >= 65_536 && !self.messages.contains_key(id) {
                return Err(AgentSessionError::Failed);
            }
            self.messages.insert(id.to_owned(), time);
        }
        if info["role"] == "user" {
            if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
                return Err(AgentSessionError::Failed);
            }
            if self.users.len() >= 4096 && !self.users.contains(id) {
                return Err(AgentSessionError::Failed);
            }
            self.users.insert(id.to_owned());
        }
        if info["role"] != "assistant" {
            return Ok(Vec::new());
        }
        let mut updates = Vec::new();
        if info["summary"] == true {
            let parent = info["parentID"].as_str().ok_or(AgentSessionError::Failed)?;
            let status = if !info["error"].is_null() {
                "failed"
            } else if info["finish"]
                .as_str()
                .is_some_and(|finish| !finish.is_empty())
            {
                "completed"
            } else {
                "started"
            };
            updates.extend(self.marker(parent, status, Some(id), None)?);
        }
        let tokens = &info["tokens"];
        let used = tokens["input"]
            .as_u64()
            .and_then(|input| input.checked_add(tokens["cache"]["read"].as_u64().unwrap_or(0)));
        let time = info["time"]["created"]
            .as_u64()
            .ok_or(AgentSessionError::Failed)?;
        let identity = (time, id.to_owned());
        if self
            .outcome
            .as_ref()
            .is_none_or(|(previous, _)| identity >= *previous)
        {
            self.outcome = Some((identity.clone(), info["finish"].as_str().map(str::to_owned)));
        }
        if let Some(used) = used.filter(|used| *used > 0)
            && self
                .latest
                .as_ref()
                .is_none_or(|previous| identity >= *previous)
            && let (Some(provider), Some(model)) =
                (info["providerID"].as_str(), info["modelID"].as_str())
            && let Some(size) = self.limits.get(&format!("{provider}/{model}"))
        {
            if !(AgentUsage {
                context_window_used_tokens: Some(used),
                context_window_max_tokens: Some(*size),
                ..AgentUsage::default()
            })
            .is_valid()
            {
                return Ok(updates);
            }
            let mut usage = json!({"sessionUpdate":"usage_update", "used":used,"size":size});
            if let Some(cost) = self.cost {
                usage["cost"] = json!({"amount":cost,"currency":"USD"});
            }
            if self.usage.as_ref() != Some(&usage) {
                updates.push(usage.clone());
            }
            self.usage = Some(usage);
            self.latest = Some(identity);
        }
        Ok(updates)
    }

    /// Session cost is cumulative native metadata, never a sum of repeated message updates.
    pub(super) fn session(&mut self, info: &Value) -> Vec<Value> {
        let Some(cost) = info["cost"]
            .as_f64()
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        else {
            return Vec::new();
        };
        self.cost = Some(cost);
        if let Some(usage) = &mut self.usage {
            let value = json!({"amount":cost,"currency":"USD"});
            if usage["cost"] != value {
                usage["cost"] = value;
                return vec![usage.clone()];
            }
        }
        Vec::new()
    }

    /// Preserve the native assistant boundary accepted before this new prompt.
    pub(super) fn begin(&mut self) {
        self.baseline = self.outcome.as_ref().map(|(id, _)| id.clone());
    }

    /// Identify a new native unknown finish that legacy ACP incorrectly labels `end_turn`.
    pub(super) fn unknown_finish(&self) -> bool {
        self.outcome.as_ref().is_some_and(|(id, finish)| {
            self.baseline.as_ref() != Some(id) && finish.as_deref() == Some("unknown")
        })
    }

    /// Native prompt completion closes any observed compaction that never reached a native terminal.
    pub(super) fn finish(&mut self) -> Result<Vec<Value>, AgentSessionError> {
        let pending: Vec<_> = self
            .markers
            .values()
            .filter(|marker| marker.status == "started")
            .map(|marker| marker.id.clone())
            .collect();
        let mut updates = Vec::new();
        for id in pending {
            updates.extend(self.marker(&id, "failed", None, None)?);
        }
        Ok(updates)
    }

    fn marker(
        &mut self,
        id: &str,
        status: &str,
        anchor: Option<&str>,
        auto: Option<bool>,
    ) -> Result<Vec<Value>, AgentSessionError> {
        if id.is_empty()
            || id.len() > 512
            || id.chars().any(char::is_control)
            || (self.markers.len() >= 4096 && !self.markers.contains_key(id))
        {
            return Err(AgentSessionError::Failed);
        }
        let marker = self.markers.entry(id.to_owned()).or_insert_with(|| Marker {
            id: id.to_owned(),
            anchor: None,
            status: String::new(),
            trigger: None,
        });
        if let Some(anchor) = anchor {
            marker.anchor = Some(anchor.to_owned());
        }
        if let Some(auto) = auto {
            marker.trigger = Some(if auto { "auto" } else { "manual" }.into());
        }
        if marker.status == status || (status == "started" && !marker.status.is_empty()) {
            return Ok(Vec::new());
        }
        status.clone_into(&mut marker.status);
        Ok(vec![
            json!({"sessionUpdate":"session_info_update", "_meta":{
            "opencode/compaction":{"messageId":id, "status":status,"reason":marker.trigger}}}),
        ])
    }

    /// Reconcile native metadata after subscription and before a prompt's terminal publication.
    pub(super) fn snapshot(&mut self, rows: &[Row]) -> Result<Vec<Value>, AgentSessionError> {
        let mut updates = Vec::new();
        for row in rows {
            for part in &row.parts {
                if part.kind == "compaction" {
                    updates.extend(self.marker(
                        row.info["id"].as_str().ok_or(AgentSessionError::Failed)?,
                        "started",
                        None,
                        part.auto,
                    )?);
                }
            }
            updates.extend(self.message(&row.info)?);
        }
        Ok(updates
            .into_iter()
            .filter(|update| update["_meta"]["opencode/compaction"]["status"] != "started")
            .collect())
    }

    /// Add native compaction facts around ACP's authoritative text/tool history, retaining its turns.
    pub(super) fn history(
        &self,
        entries: &mut Vec<NativeItem>,
        clients: &BTreeMap<String, String>,
    ) {
        let ordered: BTreeSet<_> = {
            let visible: BTreeSet<_> = entries
                .iter()
                .filter_map(|entry| entry.item["messageId"].as_str())
                .collect();
            self.messages
                .iter()
                .filter(|(id, _)| visible.contains(id.as_str()))
                .map(|(id, time)| (*time, id.as_str()))
                .collect()
        };
        let mut markers = BTreeMap::<&str, Vec<&Marker>>::new();
        for marker in self
            .markers
            .values()
            .filter(|marker| marker.status != "started")
        {
            let anchor = marker
                .anchor
                .as_deref()
                .filter(|id| {
                    self.messages
                        .get(*id)
                        .is_some_and(|time| ordered.contains(&(*time, *id)))
                })
                .or_else(|| {
                    self.messages.get(&marker.id).and_then(|time| {
                        ordered
                            .range((
                                std::ops::Bound::Excluded((*time, marker.id.as_str())),
                                std::ops::Bound::Unbounded,
                            ))
                            .next()
                            .map(|(_, id)| *id)
                    })
                })
                .or(marker.anchor.as_deref())
                .unwrap_or("");
            markers.entry(anchor).or_default().push(marker);
        }
        let mut emitted: BTreeSet<_> = entries
            .iter()
            .filter(|entry| entry.item["type"] == "compaction")
            .filter_map(|entry| entry.item["compactionId"].as_str().map(str::to_owned))
            .collect();
        let mut result = Vec::with_capacity(entries.len() + self.markers.len());
        for mut entry in entries.drain(..) {
            if let Some(id) = entry.item["messageId"].as_str()
                && let Some(markers) = markers.get(id)
            {
                for marker in markers {
                    if marker.anchor.as_deref() == Some(id)
                        && let Some(turn) = clients.get(&marker.id)
                    {
                        entry.turn_id = Some(turn.clone());
                    }
                    if emitted.insert(marker.id.clone()) {
                        result.push(marker.entry(&entry, clients));
                    }
                }
            }
            result.push(entry);
        }
        if let Some(last) = result.last().cloned() {
            for marker in self
                .markers
                .values()
                .filter(|marker| marker.status != "started")
            {
                if emitted.insert(marker.id.clone()) {
                    result.push(marker.entry(&last, clients));
                }
            }
        }
        *entries = result;
    }
}

impl Marker {
    fn entry(&self, anchor: &NativeItem, clients: &BTreeMap<String, String>) -> NativeItem {
        let mut item = json!({"type":"compaction", "compactionId":self.id,"status":self.status});
        if let Some(trigger) = &self.trigger {
            item["trigger"] = json!(trigger);
        }
        NativeItem {
            key: format!(
                "native:opencode:acp-v1:compaction:{}:{}",
                self.id, self.status
            ),
            turn_id: clients
                .get(&self.id)
                .cloned()
                .or_else(|| anchor.turn_id.clone()),
            timestamp: anchor.timestamp.clone(),
            item,
        }
    }
}
