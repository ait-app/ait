//! Bounded, connection-scoped projection of OMP's durable native side-question history.

use serde::Serialize;
use serde_json::{Value, json};

use crate::ports::agent_session::AgentSessionError;

const MAX_TOPICS: usize = 8;
const MAX_TURNS: usize = 8;
const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Debug, Default)]
pub(super) struct State {
    records: Vec<Record>,
    error: Option<String>,
    omitted: usize,
    live_running: Option<String>,
    reveal: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: String,
    question: String,
    created_at: u64,
    turns: Vec<Turn>,
    truncated: bool,
    #[serde(skip)]
    bytes: usize,
    #[serde(skip)]
    turn_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Turn {
    question: String,
    answer: String,
    status: String,
    created_at: u64,
    updated_at: u64,
    error: Option<String>,
}

fn bounded(text: &str, limit: usize) -> (&str, bool) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], end < text.len())
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key].as_str().ok_or(AgentSessionError::Failed)
}

fn timestamp(value: &Value, key: &str) -> Result<u64, AgentSessionError> {
    value[key]
        .as_u64()
        .filter(|value| *value <= 9_007_199_254_740_991)
        .ok_or(AgentSessionError::Failed)
}

fn safe_error(error: &Value) -> Option<String> {
    if error.is_null() {
        return None;
    }
    super::bridge::redact_event(&json!({"error":error}))["error"]
        .as_str()
        .map(|error| bounded(error, 4096).0.to_owned())
}

fn parse(value: &Value) -> Result<Record, AgentSessionError> {
    let id = text(value, "id")?;
    if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
        return Err(AgentSessionError::Failed);
    }
    let (question, mut truncated) = bounded(text(value, "question")?, 4096);
    let followups = value
        .get("followUps")
        .filter(|value| !value.is_null())
        .map(|value| value.as_array().ok_or(AgentSessionError::Failed))
        .transpose()?;
    let count = followups.map_or(0, Vec::len);
    truncated |= count >= MAX_TURNS;
    let mut bytes = question.len();
    let mut turns = Vec::with_capacity((count + 1).min(MAX_TURNS));
    for entry in followups
        .into_iter()
        .flat_map(|items| items.iter().rev())
        .chain(std::iter::once(value))
        .take(MAX_TURNS)
    {
        let status = text(entry, "status")?;
        if !matches!(status, "running" | "complete" | "cancelled" | "error") {
            return Err(AgentSessionError::Failed);
        }
        let (question, clipped) =
            bounded(text(entry, "question")?, 4096.min(MAX_RECORD_BYTES - bytes));
        bytes += question.len();
        truncated |= clipped;
        let (answer, clipped) = bounded(
            text(entry, "answer")?,
            (48 * 1024).min(MAX_RECORD_BYTES - bytes),
        );
        bytes += answer.len();
        truncated |= clipped;
        turns.push(Turn {
            question: question.to_owned(),
            answer: answer.to_owned(),
            status: status.to_owned(),
            created_at: timestamp(entry, "createdAt")?,
            updated_at: timestamp(entry, "updatedAt")?,
            error: safe_error(&entry["error"]),
        });
    }
    turns.reverse();
    Ok(Record {
        id: id.to_owned(),
        question: question.to_owned(),
        created_at: timestamp(value, "createdAt")?,
        turns,
        truncated,
        bytes,
        turn_count: count + 1,
    })
}

impl State {
    /// Capture native lifecycle/delta/history frames without touching foreground turn state.
    /// Returns malformed-record failures for the caller to isolate to the side-question panel.
    pub(super) fn capture(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        match event["type"].as_str() {
            Some("notice") if event["source"] == "btw-history" => {
                self.error = safe_error(&event["message"]);
            }
            Some("btw_record") => {
                self.upsert(parse(&event["record"])?, true);
            }
            Some("btw_delta") => {
                let id = text(event, "recordId")?;
                let delta = text(event, "delta")?;
                if let Some(record) = self.records.iter_mut().find(|record| record.id == id)
                    && let Some(turn) = record
                        .turns
                        .last_mut()
                        .filter(|turn| turn.status == "running")
                {
                    let limit =
                        (48 * 1024 - turn.answer.len()).min(MAX_RECORD_BYTES - record.bytes);
                    let (delta, clipped) = bounded(delta, limit);
                    turn.answer.push_str(delta);
                    record.bytes += delta.len();
                    record.truncated |= clipped;
                }
            }
            Some("response")
                if matches!(
                    event["command"].as_str(),
                    Some("btw" | "btw_cancel" | "get_btw_history")
                ) =>
            {
                if event["success"] != true {
                    self.error = safe_error(&event["error"]);
                } else if event["command"] == "btw" {
                    self.upsert(parse(&event["data"]["record"])?, true);
                } else if event["command"] == "get_btw_history" {
                    self.restore(&event["data"])?;
                    self.reveal = Some(
                        bounded(event["id"].as_str().unwrap_or("history"), 1024)
                            .0
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn upsert(&mut self, record: Record, live: bool) {
        if let Some(index) = self.records.iter().position(|old| old.id == record.id) {
            let previous = &self.records[index];
            let old = previous
                .turns
                .last()
                .expect("native records have at least one turn");
            let new = record
                .turns
                .last()
                .expect("native records have at least one turn");
            if record.turn_count < previous.turn_count
                || record.turn_count == previous.turn_count
                    && (new.created_at < old.created_at
                        || new.created_at == old.created_at
                            && (new.updated_at < old.updated_at
                                || new.updated_at == old.updated_at
                                    && new.status == "running"
                                    && (old.status != "running"
                                        || new.answer.len() < old.answer.len())))
            {
                return;
            }
            self.observe(&record, live);
            self.records[index] = record;
        } else {
            self.observe(&record, live);
            self.records.push(record);
        }
        self.error = None;
        self.records.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        if self.records.len() > MAX_TOPICS {
            self.records.truncate(MAX_TOPICS);
            self.omitted += 1;
        }
    }

    fn observe(&mut self, record: &Record, live: bool) {
        let running = record
            .turns
            .last()
            .is_some_and(|turn| turn.status == "running");
        if live && running {
            self.live_running = Some(record.id.clone());
            self.reveal = Some(format!("{}:{}", record.id, record.turn_count));
        } else if !running && self.live_running.as_deref() == Some(record.id.as_str()) {
            self.live_running = None;
        }
    }

    /// Restore native records without presenting a stale on-disk running record as live work.
    /// Returns malformed native history; the caller keeps that error in this panel only.
    pub(super) fn restore(&mut self, data: &Value) -> Result<(), AgentSessionError> {
        let records = data["records"]
            .as_array()
            .ok_or(AgentSessionError::Failed)?;
        self.omitted = self.omitted.max(records.len().saturating_sub(MAX_TOPICS));
        for record in records.iter().take(MAX_TOPICS) {
            self.upsert(parse(record)?, false);
        }
        Ok(())
    }

    /// Whether this connection observed a side question starting and not yet settling.
    pub(super) fn running(&self) -> bool {
        self.live_running.is_some()
    }

    /// Isolate malformed native side-question payloads from a healthy foreground conversation.
    pub(super) fn malformed(&mut self) {
        self.error = Some("OMP returned an invalid side-question response.".to_owned());
    }

    /// Return a bounded UI snapshot; full records remain in OMP's own history sidecar.
    pub(super) fn snapshot(&self) -> Value {
        json!({"records":self.records,"error":self.error,"omitted":self.omitted,"reveal":self.reveal,"runningId":self.live_running})
    }
}

#[cfg(test)]
mod tests;
