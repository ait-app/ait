//! Session-scoped questions are messages, not blocking native JSON-RPC requests.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::ports::agent_session::AgentSessionError;
use crate::protocol::{prompt::AgentPrompt, timeline::NativeItem};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Question {
    title: String,
    #[serde(default)]
    options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    item: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<AnswerPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Anchor {
    key: String,
    turn_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AnswerPosition {
    anchor: Option<Anchor>,
    timestamp: String,
    order: u64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Questions {
    records: BTreeMap<String, Record>,
    anchor: Option<Anchor>,
}

impl Questions {
    pub(super) fn restore(saved: Option<&Value>) -> Result<Self, AgentSessionError> {
        let Some(saved) = saved else {
            return Ok(Self::default());
        };
        let records: Vec<Record> =
            serde_json::from_value(saved.clone()).map_err(|_| AgentSessionError::Failed)?;
        let mut questions = Self::default();
        for record in records {
            questions.receive(&record.item)?;
            let id = request_id(&record.item)?;
            if let Some(resolution) = &record.resolution {
                let count = parse(&record.item)?.len();
                if resolution != "dismissed"
                    && !resolution.as_array().is_some_and(|answers| {
                        answers.len() == count
                            && answers.iter().all(|answer| {
                                answer.as_str().is_some_and(|answer| {
                                    !answer.trim().is_empty() && answer.len() <= 8192
                                })
                            })
                    })
                {
                    return Err(AgentSessionError::Failed);
                }
            }
            if record.position.as_ref().is_some_and(|position| {
                record.resolution.is_none()
                    || position.order == 0
                    || DateTime::parse_from_rfc3339(&position.timestamp).is_err()
                    || position.anchor.as_ref().is_some_and(|anchor| {
                        anchor.key.is_empty()
                            || anchor.turn_id.as_ref().is_some_and(String::is_empty)
                    })
            }) {
                return Err(AgentSessionError::Failed);
            }
            questions.records.insert(id, record);
        }
        questions.check_size()?;
        Ok(questions)
    }

    pub(super) fn saved(&self) -> Option<Value> {
        (!self.records.is_empty()).then(|| json!(self.records.values().collect::<Vec<_>>()))
    }

    pub(super) fn receive(&mut self, item: &Value) -> Result<Option<Value>, AgentSessionError> {
        parse(item)?;
        let id = request_id(item)?;
        if let Some(record) = self.records.get(&id) {
            return if record.item == *item {
                Ok(None)
            } else {
                Err(AgentSessionError::Failed)
            };
        }
        if self.records.len() >= 128 || self.pending().len() >= 32 {
            return Err(AgentSessionError::Failed);
        }
        let record = Record {
            item: item.clone(),
            resolution: None,
            position: None,
        };
        let permission = permission(&record)?;
        self.records.insert(id.clone(), record);
        if let Err(error) = self.check_size() {
            self.records.remove(&id);
            return Err(error);
        }
        Ok(Some(permission))
    }

    fn check_size(&self) -> Result<(), AgentSessionError> {
        if serde_json::to_vec(&self.saved())
            .map_err(|_| AgentSessionError::Failed)?
            .len()
            > 128 * 1024
        {
            return Err(AgentSessionError::Failed);
        }
        Ok(())
    }

    pub(super) fn pending(&self) -> Vec<Value> {
        self.records
            .values()
            .filter(|record| record.resolution.is_none())
            .filter_map(|record| permission(record).ok())
            .collect()
    }

    pub(super) fn contains(&self, id: &str) -> bool {
        self.records.contains_key(id)
    }

    /// Remember the latest native display item as an anchor for subsequent answers.
    /// `entry` supplies its stable identity and turn; its payload is not retained.
    pub(super) fn observe(&mut self, entry: &NativeItem) {
        self.anchor = Some(Anchor {
            key: entry.key.clone(),
            turn_id: entry.turn_id.clone(),
        });
    }

    pub(super) fn prepare(
        &self,
        id: &str,
        response: &Value,
    ) -> Result<Option<AgentPrompt>, AgentSessionError> {
        let record = self
            .records
            .get(id)
            .filter(|record| record.resolution.is_none())
            .ok_or(AgentSessionError::Rejected)?;
        let resolution = resolution(record, response)?;
        let mut candidate = self.clone();
        candidate.resolve(id, response)?;
        let Some(answers) = resolution.as_array() else {
            return Ok(None);
        };
        let mut text = "Answers to your questions:\n\n".to_owned();
        for (index, question) in parse(&record.item)?.iter().enumerate() {
            if index > 0 {
                text.push_str("\n\n");
            }
            write!(
                text,
                "{}\n{}",
                question.title,
                answers[index].as_str().unwrap_or_default()
            )
            .map_err(|_| AgentSessionError::Failed)?;
        }
        let prompt = AgentPrompt {
            text,
            client_message_id: Some(answer_message_id(id)),
            ..AgentPrompt::default()
        };
        prompt.validate()?;
        Ok(Some(prompt))
    }

    pub(super) fn resolve(
        &mut self,
        id: &str,
        response: &Value,
    ) -> Result<NativeItem, AgentSessionError> {
        let order = self
            .records
            .values()
            .filter_map(|record| record.position.as_ref().map(|position| position.order))
            .max()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or(AgentSessionError::Failed)?;
        let record = self
            .records
            .get_mut(id)
            .ok_or(AgentSessionError::Rejected)?;
        if record.resolution.is_some() {
            return Err(AgentSessionError::Rejected);
        }
        let answer = resolution(record, response)?;
        record.resolution = Some(answer);
        record.position = Some(AnswerPosition {
            anchor: self.anchor.clone(),
            timestamp: Utc::now().to_rfc3339(),
            order,
        });
        let entry = answer_entry(record, None)?;
        if let Err(error) = self.check_size() {
            if let Some(record) = self.records.get_mut(id) {
                record.resolution = None;
                record.position = None;
            }
            return Err(error);
        }
        Ok(entry)
    }

    pub(super) fn history(&self, entries: &mut Vec<NativeItem>) -> Result<(), AgentSessionError> {
        if !self
            .records
            .values()
            .any(|record| record.resolution.is_some())
        {
            return Ok(());
        }
        let keys: HashMap<_, _> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.key.as_str(), index))
            .collect();
        let calls: HashMap<_, _> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| entry.item["callId"].as_str().map(|id| (id, index)))
            .collect();
        let prompts: HashMap<_, _> = entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry.item["clientMessageId"].as_str().map(|id| (id, index))
            })
            .collect();
        let mut insertions = BTreeMap::<usize, Vec<_>>::new();
        for (id, record) in self
            .records
            .iter()
            .filter(|(_, record)| record.resolution.is_some())
        {
            let Some(question) = record.item["id"].as_str().and_then(|id| calls.get(id)) else {
                continue;
            };
            let answer = answer_entry(record, Some(&entries[*question]))?;
            if keys.contains_key(answer.key.as_str()) {
                continue;
            }
            let index = record
                .position
                .as_ref()
                .and_then(|position| position.anchor.as_ref())
                .and_then(|anchor| keys.get(anchor.key.as_str()))
                .map(|index| index + 1)
                .or_else(|| prompts.get(answer_message_id(id).as_str()).copied())
                .unwrap_or(question + 1)
                .max(question + 1);
            insertions.entry(index).or_default().push((
                record.position.as_ref().map(|position| position.order),
                answer,
            ));
        }
        merge_answers(entries, insertions);
        Ok(())
    }

    pub(super) fn retain(&mut self, entries: &[NativeItem]) {
        self.records.retain(|_, record| {
            entries
                .iter()
                .any(|entry| entry.item["callId"] == record.item["id"])
        });
    }
}

fn answer_message_id(id: &str) -> String {
    format!("async-answer:{:x}", Sha256::digest(id))
}

fn merge_answers(
    entries: &mut Vec<NativeItem>,
    insertions: BTreeMap<usize, Vec<(Option<u64>, NativeItem)>>,
) {
    if insertions.is_empty() {
        return;
    }
    let mut source = std::mem::take(entries).into_iter();
    entries.reserve(source.len() + insertions.values().map(Vec::len).sum::<usize>());
    let mut offset = 0;
    for (index, mut answers) in insertions {
        entries.extend(source.by_ref().take(index - offset));
        offset = index;
        answers.sort_by_key(|(order, _)| *order);
        entries.extend(answers.into_iter().map(|(_, entry)| entry));
    }
    entries.extend(source);
}

fn request_id(item: &Value) -> Result<String, AgentSessionError> {
    let id = item["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or(AgentSessionError::Failed)?;
    Ok(format!("permission-{id}"))
}

fn parse(item: &Value) -> Result<Vec<Question>, AgentSessionError> {
    if item["type"] != "agentMessage" || item["delivery"] != "async" {
        return Err(AgentSessionError::Failed);
    }
    request_id(item)?;
    let questions: Vec<Question> =
        serde_json::from_value(item["questions"].clone()).map_err(|_| AgentSessionError::Failed)?;
    if questions.is_empty()
        || questions.len() > 32
        || questions.iter().any(|question| {
            question.title.trim().is_empty()
                || question.title.len() > 8192
                || question.options.as_ref().is_some_and(|options| {
                    options.len() > 32
                        || options
                            .iter()
                            .any(|option| option.is_empty() || option.len() > 2048)
                })
        })
    {
        return Err(AgentSessionError::Failed);
    }
    Ok(questions)
}

fn permission(record: &Record) -> Result<Value, AgentSessionError> {
    let questions: Vec<_> = parse(&record.item)?.iter().enumerate().map(|(index, question)| {
        json!({"id":index.to_string(),"header":format!("Question {}", index + 1),
            "question":question.title,"isOther":true,"options":question.options.as_deref().unwrap_or_default()
                .iter().map(|label| json!({"label":label})).collect::<Vec<_>>()})
    }).collect();
    Ok(
        json!({"id":request_id(&record.item)?,"provider":"codex","name":"request_user_input_async",
        "kind":"question","title":"Question","input":{"questions":questions}}),
    )
}

fn resolution(record: &Record, response: &Value) -> Result<Value, AgentSessionError> {
    match response["behavior"].as_str() {
        Some("deny") => Ok(json!("dismissed")),
        Some("allow") => {
            let questions = parse(&record.item)?;
            let answers = response["updatedInput"]["answers"]
                .as_object()
                .filter(|answers| answers.len() == questions.len())
                .ok_or(AgentSessionError::Rejected)?;
            let values = (0..questions.len())
                .map(|index| {
                    answers
                        .get(&format!("Question {}", index + 1))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|answer| !answer.is_empty() && answer.len() <= 8192)
                        .map(str::to_owned)
                        .ok_or(AgentSessionError::Rejected)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!(values))
        }
        _ => Err(AgentSessionError::Rejected),
    }
}

pub(super) fn timeline(item: &Value) -> Result<Value, AgentSessionError> {
    let text = parse(item)?
        .iter()
        .map(|question| {
            format!(
                "{}\n{}",
                question.title,
                question.options.as_deref().unwrap_or_default().join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok(
        json!({"type":"tool_call","callId":item["id"],"name":"request_user_input_async",
        "status":"completed","error":null,"detail":{"type":"plain_text","icon":"brain","text":text}}),
    )
}

fn answer_entry(
    record: &Record,
    question: Option<&NativeItem>,
) -> Result<NativeItem, AgentSessionError> {
    let mut item = timeline(&record.item)?;
    let text = if record.resolution.as_ref() == Some(&json!("dismissed")) {
        "Question dismissed".to_owned()
    } else {
        let answers = record
            .resolution
            .as_ref()
            .and_then(Value::as_array)
            .ok_or(AgentSessionError::Failed)?;
        parse(&record.item)?
            .iter()
            .zip(answers)
            .map(|(question, answer)| {
                format!(
                    "{}\n{}",
                    question.title,
                    answer.as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    item["callId"] = json!(format!(
        "{}:answer",
        record.item["id"]
            .as_str()
            .ok_or(AgentSessionError::Failed)?
    ));
    item["detail"]["text"] = json!(text);
    Ok(NativeItem {
        key: format!("native:async-answer:{}", request_id(&record.item)?),
        turn_id: if let Some(position) = &record.position {
            position
                .anchor
                .as_ref()
                .and_then(|anchor| anchor.turn_id.clone())
        } else {
            question.and_then(|entry| entry.turn_id.clone())
        },
        timestamp: record
            .position
            .as_ref()
            .map(|position| position.timestamp.clone())
            .or_else(|| question.map(|entry| entry.timestamp.clone()))
            .ok_or(AgentSessionError::Failed)?,
        item,
    })
}

#[cfg(test)]
mod tests;
