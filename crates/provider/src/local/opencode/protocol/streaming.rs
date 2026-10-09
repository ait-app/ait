//! Native event codecs materialize text only after confirming assistant ownership.
use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use super::{OpenCodeExecutionLimits, Version, failure, http::required_string};
use crate::local::opencode::types::{Fault, ProgressEvent, ProtocolError};

#[derive(Debug)]
struct Part {
    message: String,
    text: String,
    emitted: usize,
}

#[derive(Debug)]
pub(in crate::local::opencode) struct Stream {
    version: Version,
    session: String,
    roles: HashMap<String, bool>,
    parts: BTreeMap<String, Part>,
    bytes: usize,
    limits: OpenCodeExecutionLimits,
}

impl Stream {
    #[cfg(test)]
    pub(in crate::local::opencode) fn new(
        session: &str,
        input: &str,
        limits: OpenCodeExecutionLimits,
    ) -> Self {
        Self::for_protocol(Version::V1, session, input, limits)
    }

    pub(in crate::local::opencode) fn for_protocol(
        version: Version,
        session: &str,
        input: &str,
        limits: OpenCodeExecutionLimits,
    ) -> Self {
        Self {
            version,
            session: session.into(),
            roles: HashMap::from([(input.into(), false)]),
            parts: BTreeMap::new(),
            bytes: 0,
            limits,
        }
    }

    #[cfg(test)]
    fn observe_versioned(
        &mut self,
        raw: &Value,
        version: Version,
    ) -> Result<Vec<ProgressEvent>, ProtocolError> {
        self.version = version;
        self.observe(raw)
    }

    pub(in crate::local::opencode) fn observe(
        &mut self,
        raw: &Value,
    ) -> Result<Vec<ProgressEvent>, ProtocolError> {
        let event = raw.get("payload").unwrap_or(raw);
        let data = event
            .get("properties")
            .or_else(|| event.get("data"))
            .unwrap_or(&Value::Null);
        let owner = data
            .get("sessionID")
            .or_else(|| data.pointer("/part/sessionID"))
            .or_else(|| data.pointer("/info/sessionID"))
            .and_then(Value::as_str);
        if owner != Some(&self.session) {
            return Ok(Vec::new());
        }
        match (self.version, event["type"].as_str()) {
            (Version::V1, Some("message.updated")) => self.message(&data["info"]),
            (Version::V1, Some("message.part.updated")) if data["part"]["type"] == "text" => {
                self.updated(&data["part"])
            }
            (Version::V1, Some("message.part.delta")) if data["field"] == "text" => {
                self.delta(data)
            }
            (Version::V2, Some("session.text.delta")) => {
                let id = required_string(data, "assistantMessageID")?;
                let delta = text(data, "delta")?;
                self.check(delta.len(), false, false)?;
                self.bytes += delta.len();
                Ok(if delta.is_empty() {
                    Vec::new()
                } else {
                    vec![ProgressEvent::TextDelta {
                        id: format!("{id}:0"),
                        delta: delta.into(),
                    }]
                })
            }
            _ => Ok(Vec::new()),
        }
    }

    fn message(&mut self, info: &Value) -> Result<Vec<ProgressEvent>, ProtocolError> {
        let id = required_string(info, "id")?;
        let assistant = match info["role"].as_str() {
            Some("assistant") => true,
            Some("user") => false,
            _ => return Ok(Vec::new()),
        };
        if self
            .roles
            .get(id)
            .is_some_and(|previous| *previous != assistant)
        {
            return Err(failure(
                Fault::RunRecoveryFailed,
                "native message changed role",
            ));
        }
        self.check(0, false, !self.roles.contains_key(id))?;
        self.roles.insert(id.into(), assistant);
        if !assistant {
            self.parts.retain(|_, part| {
                if part.message == id {
                    self.bytes -= part.text.len();
                    false
                } else {
                    true
                }
            });
            return Ok(Vec::new());
        }
        Ok(self
            .parts
            .iter_mut()
            .filter(|(_, part)| part.message == id)
            .filter_map(|(id, part)| emit(id, part))
            .collect())
    }

    fn updated(&mut self, value: &Value) -> Result<Vec<ProgressEvent>, ProtocolError> {
        let message = required_string(value, "messageID")?;
        if self.roles.get(message) == Some(&false) {
            return Ok(Vec::new());
        }
        let id = required_string(value, "id")?;
        let text = text(value, "text")?;
        let previous = self.parts.get(id);
        if let Some(part) = previous {
            if part.message != message {
                return Err(failure(Fault::ProviderFailed, "native part changed parent"));
            }
            // Late placeholders and cumulative snapshots may lag already received deltas.
            if part.text.starts_with(text) {
                return Ok(Vec::new());
            }
            if !text.starts_with(&part.text) {
                return Err(failure(
                    Fault::RunRecoveryFailed,
                    "native text changed published prefix",
                ));
            }
        }
        let previous_len = previous.map_or(0, |part| part.text.len());
        self.check(text.len() - previous_len, previous.is_none(), false)?;
        let part = self.parts.entry(id.into()).or_insert_with(|| Part {
            message: message.into(),
            text: String::new(),
            emitted: 0,
        });
        self.bytes += text.len() - previous_len;
        text.clone_into(&mut part.text);
        Ok(if self.roles.get(message) == Some(&true) {
            emit(id, part).into_iter().collect()
        } else {
            Vec::new()
        })
    }

    fn delta(&mut self, value: &Value) -> Result<Vec<ProgressEvent>, ProtocolError> {
        let message = required_string(value, "messageID")?;
        if self.roles.get(message) == Some(&false) {
            return Ok(Vec::new());
        }
        let id = required_string(value, "partID")?;
        let delta = text(value, "delta")?;
        // Without the part type, a text-field delta could also be reasoning. Reconcile from
        // its later complete part or history rather than guessing the display item type.
        let Some(part) = self.parts.get(id) else {
            return Ok(Vec::new());
        };
        if part.message != message {
            return Err(failure(
                Fault::ProviderFailed,
                "native delta changed parent",
            ));
        }
        self.check(delta.len(), false, false)?;
        let part = self
            .parts
            .get_mut(id)
            .expect("checked native text part exists");
        self.bytes += delta.len();
        part.text.push_str(delta);
        Ok(if self.roles.get(message) == Some(&true) {
            emit(id, part).into_iter().collect()
        } else {
            Vec::new()
        })
    }

    fn check(&self, added: usize, new_part: bool, new_message: bool) -> Result<(), ProtocolError> {
        if self.bytes.saturating_add(added) > self.limits.max_output_bytes
            || self.parts.len() as u64 + u64::from(new_part) > self.limits.max_steps
            || self.roles.len() as u64 + u64::from(new_message)
                > self.limits.max_steps.saturating_add(1)
        {
            return Err(failure(
                Fault::RunLimitExceeded,
                "native text observation exceeded its limits",
            ));
        }
        Ok(())
    }
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, ProtocolError> {
    value[field]
        .as_str()
        .ok_or_else(|| failure(Fault::ProviderFailed, "native text must be a string"))
}

fn emit(id: &str, part: &mut Part) -> Option<ProgressEvent> {
    let delta = &part.text[part.emitted..];
    if delta.is_empty() {
        return None;
    }
    let event = ProgressEvent::TextDelta {
        id: id.into(),
        delta: delta.into(),
    };
    part.emitted = part.text.len();
    Some(event)
}

#[cfg(test)]
mod tests;
