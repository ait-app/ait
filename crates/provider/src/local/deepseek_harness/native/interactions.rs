//! Exact-session Host waterfalls become bounded approvals and structured questions.
use super::super::{PROVIDER, config::text};
use crate::ports::agent_session::AgentSessionError;
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Debug)]
pub(super) struct Pending {
    pub(super) event_id: String,
    pub(super) request: Value,
    native_questions: Option<Vec<Value>>,
}

impl Pending {
    /// Capture one supported waterfall; foreign-agent routing is checked by the owning session.
    pub(super) fn capture(frame: &Value, tool: Option<&Value>) -> Result<Self, AgentSessionError> {
        if frame.to_string().len() > 65536 {
            return Err(AgentSessionError::Failed);
        }
        let event_id = text(frame, "eventId")?.to_owned();
        let native = &frame["request"];
        let id = uuid::Uuid::new_v4().to_string();
        let (name, kind, input, questions) = match frame["event"].as_str() {
            Some("user-questions/request") => {
                let questions = native["questions"]
                    .as_array()
                    .filter(|questions| !questions.is_empty() && questions.len() <= 32)
                    .ok_or(AgentSessionError::Failed)?;
                let mut ids = BTreeSet::new();
                for question in questions {
                    if !ids.insert(text(question, "id")?) {
                        return Err(AgentSessionError::Failed);
                    }
                    question["question"]
                        .as_str()
                        .filter(|value| !value.trim().is_empty() && value.len() <= 16384)
                        .ok_or(AgentSessionError::Failed)?;
                    if question
                        .get("multiSelect")
                        .is_some_and(|value| !value.is_boolean())
                    {
                        return Err(AgentSessionError::Failed);
                    }
                    if let Some(options) = question.get("options") {
                        let options = options
                            .as_array()
                            .filter(|options| options.len() <= 128)
                            .ok_or(AgentSessionError::Failed)?;
                        let mut labels = BTreeSet::new();
                        for option in options {
                            if !labels.insert(text(option, "label")?) {
                                return Err(AgentSessionError::Failed);
                            }
                        }
                    }
                }
                (
                    "ask_user".to_owned(),
                    "question",
                    json!({"questions": questions.iter().map(|question| json!({
                        "question":question.get("detail").and_then(Value::as_str).map_or_else(||question["question"].clone(), |detail|json!(format!("{}\n\n{detail}",question["question"].as_str().unwrap_or_default()))),
                        "header":question.get("header").unwrap_or(&question["id"]),"answerKey":question["id"],
                        "options":question.get("options").cloned().unwrap_or_else(||json!([])),
                        "multiSelect":question["multiSelect"]==true,"allowOther":true,"answerFormat":"array"
                    })).collect::<Vec<_>>()}),
                    Some(questions.clone()),
                )
            }
            Some("approval/request") => {
                let name = text(native, "toolName")?.to_owned();
                let input = tool
                    .filter(|tool| tool["name"] == name)
                    .and_then(|tool| tool.get("input"))
                    .cloned()
                    .or_else(|| {
                        native
                            .get("callId")
                            .is_none()
                            .then(|| json!({"reason":native["reason"]}))
                    })
                    .ok_or(AgentSessionError::Failed)?;
                (name, "tool", input, None)
            }
            _ => return Err(AgentSessionError::Unavailable),
        };
        let mut request = json!({"id":id,"provider":PROVIDER,"name":name,"kind":kind,"input":input,
            "actions":[{"id":"allow","label":"Allow once","behavior":"allow"},{"id":"deny","label":"Deny","behavior":"deny"}]});
        if let Some(reason) = native["reason"].as_str() {
            request["description"] = json!(reason);
        }
        Ok(Self {
            event_id,
            request,
            native_questions: questions,
        })
    }

    /// Validate the existing Ait permission response and encode a native waterfall outcome.
    pub(super) fn resolve(&self, response: &Value) -> Result<Value, AgentSessionError> {
        let fields = response.as_object().ok_or(AgentSessionError::Rejected)?;
        if fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "behavior" | "updatedInput" | "selectedActionId" | "interrupt" | "message"
            )
        }) || response.to_string().len() > 65536
            || response
                .get("interrupt")
                .is_some_and(|value| !value.is_boolean())
        {
            return Err(AgentSessionError::Rejected);
        }
        let allow = match response["behavior"].as_str() {
            Some("allow") => true,
            Some("deny") => false,
            _ => return Err(AgentSessionError::Rejected),
        };
        if response
            .get("selectedActionId")
            .is_some_and(|id| id != if allow { "allow" } else { "deny" })
        {
            return Err(AgentSessionError::Rejected);
        }
        let Some(questions) = &self.native_questions else {
            if response
                .get("updatedInput")
                .is_some_and(|input| input != &self.request["input"])
            {
                return Err(AgentSessionError::Rejected);
            }
            return Ok(
                json!({"kind":"result","value":if allow {"allowed-once"} else {"rejected"}}),
            );
        };
        if !allow {
            return Ok(
                json!({"kind":"rejected","error":{"name":"AbortError","message":"User declined the question"}}),
            );
        }
        self.answers(response, questions)
    }

    fn answers(&self, response: &Value, questions: &[Value]) -> Result<Value, AgentSessionError> {
        let input = response["updatedInput"]
            .as_object()
            .ok_or(AgentSessionError::Rejected)?;
        if input
            .iter()
            .any(|(key, value)| key != "answers" && self.request["input"].get(key) != Some(value))
        {
            return Err(AgentSessionError::Rejected);
        }
        let supplied = input
            .get("answers")
            .and_then(Value::as_object)
            .filter(|answers| answers.len() == questions.len())
            .ok_or(AgentSessionError::Rejected)?;
        let mut used = BTreeSet::new();
        let mut answers = Vec::with_capacity(questions.len());
        for question in questions {
            let (key, value) = [
                question["id"].as_str(),
                question["header"].as_str(),
                question["question"].as_str(),
            ]
            .into_iter()
            .flatten()
            .find_map(|key| supplied.get_key_value(key))
            .ok_or(AgentSessionError::Rejected)?;
            if !used.insert(key) {
                return Err(AgentSessionError::Rejected);
            }
            let values = if let Some(value) = value.as_str() {
                vec![value]
            } else {
                value
                    .as_array()
                    .ok_or(AgentSessionError::Rejected)?
                    .iter()
                    .map(|value| value.as_str().ok_or(AgentSessionError::Rejected))
                    .collect::<Result<Vec<_>, _>>()?
            };
            if values.is_empty()
                || values.len() > 32
                || values
                    .iter()
                    .any(|value| value.trim().is_empty() || value.len() > 4096)
                || (question["multiSelect"] != true && values.len() != 1)
            {
                return Err(AgentSessionError::Rejected);
            }
            let mut selected = Vec::new();
            let mut custom = Vec::new();
            let mut unique = BTreeSet::new();
            for value in values {
                if !unique.insert(value) {
                    return Err(AgentSessionError::Rejected);
                }
                if question["options"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(|option| option["label"] == value))
                {
                    selected.push(value);
                } else {
                    custom.push(value);
                }
            }
            let mut answer = json!({"id":question["id"],"selected":selected});
            if !custom.is_empty() {
                answer["custom"] = json!(custom.join("\n"));
            }
            answers.push(answer);
        }
        Ok(json!({"kind":"result","value":{"answers":answers}}))
    }
}
