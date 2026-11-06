//! Native host capability handshake and the adapter's Vibe frames.
//!
//! `ait_native_host_hello` is consumed by the transport reader; `ait_vibe_state` and
//! `ait_vibe_worker` are folded into a bounded outbox and projected here into host state.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ports::agent_session::AgentTurnEvent;
use crate::ports::controls::{NativeSubagent, SubagentEvent};

/// Bridge protocol version this adapter understands.
const BRIDGE_VERSION: u32 = 1;
/// Native descendants retained per connection; the adapter never exceeds this.
const MAX_WORKERS: usize = 256;

/// Capabilities announced by a native host hello frame.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Capability {
    pub(super) bridge_version: u32,
    pub(super) sdk_version: String,
    pub(super) vibe: bool,
}

/// Decode a native host hello frame, or `None` when the frame is not one or is malformed.
/// A bridge version this adapter does not speak keeps the handshake decodable but disables vibe.
pub(super) fn hello(event: &Value) -> Option<Capability> {
    if event["type"] != "ait_native_host_hello" {
        return None;
    }
    let bridge_version = u32::try_from(event["bridgeVersion"].as_u64()?).ok()?;
    let sdk_version = event["sdkVersion"].as_str()?.to_owned();
    let vibe = event["capabilities"]["vibe"].as_bool()?;
    Some(Capability {
        bridge_version,
        sdk_version,
        vibe: vibe && bridge_version == BRIDGE_VERSION,
    })
}

/// Adapter frames the bridge consumes itself instead of forwarding to the session owner.
pub(super) fn is_frame(value: &Value) -> bool {
    matches!(
        value["type"].as_str(),
        Some("ait_vibe_state" | "ait_vibe_worker")
    )
}

/// Fold one adapter frame into the bounded outbox: at most one state frame (the newest) and
/// at most [`MAX_WORKERS`] worker frames, upserted by `worker.id`.
/// Returns whether the frame was retained; unknown or malformed frames are dropped.
pub(super) fn store(frames: &mut Vec<Value>, value: &Value) -> bool {
    match value["type"].as_str() {
        Some("ait_vibe_state") => {
            frames.retain(|frame| frame["type"] != "ait_vibe_state");
            frames.push(value.clone());
            true
        }
        Some("ait_vibe_worker") => {
            let Some(id) = value["worker"]["id"].as_str().filter(|id| !id.is_empty()) else {
                return false;
            };
            if let Some(existing) = frames
                .iter_mut()
                .find(|frame| frame["type"] == "ait_vibe_worker" && frame["worker"]["id"] == id)
            {
                *existing = value.clone();
                return true;
            }
            if frames
                .iter()
                .filter(|frame| frame["type"] == "ait_vibe_worker")
                .count()
                >= MAX_WORKERS
            {
                return false;
            }
            frames.push(value.clone());
            true
        }
        _ => false,
    }
}

/// Clip an optional native string field to `max` bytes on a character boundary.
fn preview(value: &Value, max: usize) -> Value {
    let Some(text) = value.as_str() else {
        return Value::Null;
    };
    if text.len() <= max {
        return json!(text);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    json!(text[..end])
}

/// Vibe mode and the verified native descendants it owns, scoped to one native connection.
/// A metadata probe never builds this state: only a real session consumes adapter frames.
#[derive(Debug, Default)]
pub(super) struct State {
    /// The native host greeted this connection and announced the Vibe bridge.
    pub(super) available: bool,
    /// The native host currently owns a Vibe session.
    pub(super) enabled: bool,
    pub(super) workers: BTreeMap<String, NativeSubagent>,
}

impl State {
    /// Whether the native host announced and currently owns a Vibe session.
    pub(super) fn snapshot(&self) -> Value {
        let workers: Vec<_> = self
            .workers
            .values()
            .take(MAX_WORKERS)
            .map(|worker| {
                json!({
                    "id": worker.id,
                    "parentId": worker.parent_id,
                    "status": worker.descriptor["status"],
                    "description": preview(&worker.descriptor["description"], 4096),
                    "model": preview(&worker.descriptor["model"], 512),
                })
            })
            .collect();
        json!({"enabled": self.enabled, "workers": workers})
    }

    /// Apply one adapter frame. Returns the child upsert the session owner must publish.
    ///
    /// `root` is the native session id of this connection, which OMP's own subagent contract
    /// compares against the session's persistence handle. A descendant's parent must therefore
    /// be either that root or a worker whose header UUID was already verified here; a frame
    /// naming any other parent is dropped rather than trusting an unverified native UUID.
    pub(super) fn capture(
        &mut self,
        frame: &Value,
        root: &str,
        cwd: &str,
    ) -> Option<AgentTurnEvent> {
        match frame["type"].as_str() {
            Some("ait_vibe_state") => {
                self.enabled = frame["state"]["enabled"].as_bool()?;
                None
            }
            Some("ait_vibe_worker") => {
                let worker = self.worker(&frame["worker"], root, cwd)?;
                // A worker with no terminal-or-running outcome yet stays visible in the panel
                // but cannot be published: the shared subagent wire enum has no "unknown".
                let event = (worker.descriptor["status"] != "unknown")
                    .then(|| AgentTurnEvent::Subagent(SubagentEvent::Upsert(worker.clone())));
                self.workers.insert(worker.id.clone(), worker);
                event
            }
            _ => None,
        }
    }

    fn worker(&self, source: &Value, root: &str, cwd: &str) -> Option<NativeSubagent> {
        let id = source["id"].as_str()?;
        if uuid::Uuid::parse_str(id).ok()?.to_string() != id {
            return None;
        }
        let parent = source["parentId"].as_str()?;
        if parent != root && !self.workers.contains_key(parent) {
            return None;
        }
        let status = match source["status"].as_str() {
            Some("running") => "running",
            Some("completed") => "completed",
            Some("failed") => "failed",
            Some("canceled" | "aborted") => "canceled",
            _ => "unknown",
        };
        if !self.workers.contains_key(id) && self.workers.len() >= MAX_WORKERS {
            return None;
        }
        // `title` and `toolCallId` are required (nullable) members of the shared descriptor.
        let mut descriptor = json!({
            "id": id,
            "provider": super::PROVIDER,
            "cwd": source["cwd"].as_str().unwrap_or(cwd),
            "status": status,
            "title": source["description"].as_str(),
            "toolCallId": source["toolCallId"],
            "createdAt": source["createdAt"],
            "updatedAt": source["updatedAt"],
        });
        for key in ["description", "model", "modelRole", "thinkingLevel"] {
            if let Some(value) = source.get(key).filter(|value| !value.is_null()) {
                descriptor[key] = value.clone();
            }
        }
        Some(NativeSubagent {
            persistence: None,
            id: id.to_owned(),
            parent_id: parent.to_owned(),
            cwd: source["cwd"].as_str().unwrap_or(cwd).to_owned(),
            descriptor,
        })
    }
}

#[cfg(test)]
mod tests;
