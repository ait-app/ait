use std::collections::BTreeSet;

use serde_json::{Value, json};
use uuid::Uuid;

use super::PROVIDER;
use crate::local::acp::{permissions, text};
use crate::ports::agent_session::AgentSessionError;

/// One bounded Cursor callback awaiting an explicit user decision.
#[derive(Debug)]
pub(super) struct Pending {
    pub(super) rpc_id: Value,
    pub(super) request: Value,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Tool(permissions::Pending),
    Question(Vec<Value>),
    Plan,
}

impl Pending {
    /// Project a native `message` into a pending approval; reject invalid or unsupported callbacks.
    pub(super) fn capture(message: &Value) -> Result<Self, AgentSessionError> {
        if message.to_string().len() > 65536 {
            return Err(AgentSessionError::Failed);
        }
        let rpc_id = message
            .get("id")
            .filter(|id| id.is_string() || id.is_number())
            .ok_or(AgentSessionError::Failed)?
            .clone();
        if message["method"] == "session/request_permission" {
            let pending = permissions::capture(PROVIDER, message)?;
            return Ok(Self {
                rpc_id,
                request: pending.request.clone(),
                kind: Kind::Tool(pending),
            });
        }
        let params = &message["params"];
        text(params, "toolCallId")?;
        let (kind, input, name, request_kind) = match message["method"].as_str() {
            Some("cursor/ask_question") => {
                let questions = params["questions"]
                    .as_array()
                    .filter(|questions| !questions.is_empty() && questions.len() <= 32)
                    .ok_or(AgentSessionError::Failed)?;
                let mut ids = BTreeSet::new();
                let mut projected = Vec::with_capacity(questions.len());
                for question in questions {
                    let id = text(question, "id")?;
                    if !ids.insert(id)
                        || question
                            .get("allowMultiple")
                            .is_some_and(|value| !value.is_boolean())
                    {
                        return Err(AgentSessionError::Failed);
                    }
                    let prompt = question["prompt"]
                        .as_str()
                        .filter(|text| !text.is_empty() && text.len() <= 16384)
                        .ok_or(AgentSessionError::Failed)?;
                    let options = question["options"]
                        .as_array()
                        .filter(|options| !options.is_empty() && options.len() <= 128)
                        .ok_or(AgentSessionError::Failed)?;
                    let mut option_ids = BTreeSet::new();
                    let mut labels = BTreeSet::new();
                    for option in options {
                        if !option_ids.insert(text(option, "id")?)
                            || !labels.insert(text(option, "label")?)
                        {
                            return Err(AgentSessionError::Failed);
                        }
                    }
                    projected.push(json!({"answerKey":id,"header":id,"question":prompt,
                        "options":options.iter().map(|option| json!({"label":option["label"]})).collect::<Vec<_>>(),
                        "multiSelect":question["allowMultiple"]==true,"allowOther":false,"answerFormat":"array"}));
                }
                (
                    Kind::Question(questions.clone()),
                    json!({"questions":projected}),
                    "ask_user",
                    "question",
                )
            }
            Some("cursor/create_plan") => {
                let plan = params["plan"]
                    .as_str()
                    .filter(|plan| !plan.is_empty())
                    .ok_or(AgentSessionError::Failed)?;
                (
                    Kind::Plan,
                    json!({"plan":plan,"name":params["name"],"overview":params["overview"],"todos":params["todos"]}),
                    "approve_plan",
                    "plan",
                )
            }
            _ => return Err(AgentSessionError::Unavailable),
        };
        Ok(Self {
            rpc_id,
            request: json!({"id":Uuid::new_v4().to_string(),"provider":PROVIDER,
                "name":name,"kind":request_kind,"input":input,
                "title":params["title"].as_str().unwrap_or(name),
                "actions":[{"id":"allow","label":"Approve","behavior":"allow"},
                    {"id":"deny","label":"Reject","behavior":"deny"}]}),
            kind,
        })
    }

    /// Resolve this callback as cancelled without authorizing its operation.
    pub(super) fn cancelled(&self) -> Value {
        json!({"jsonrpc":"2.0","id":self.rpc_id,"result":{"outcome":{"outcome":"cancelled"}}})
    }

    /// Encode an explicit Ait `response` for this callback; reject invalid choices or input changes.
    pub(super) fn resolve(&self, response: &Value) -> Result<Value, AgentSessionError> {
        if let Kind::Tool(pending) = &self.kind {
            return permissions::resolve(pending, response);
        }
        let fields = response.as_object().ok_or(AgentSessionError::Rejected)?;
        if response.to_string().len() > 65536
            || fields.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "behavior" | "updatedInput" | "selectedActionId" | "interrupt" | "message"
                )
            })
            || response
                .get("interrupt")
                .is_some_and(|value| !value.is_boolean())
            || response
                .get("message")
                .is_some_and(|value| !value.is_string())
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
        let outcome = match &self.kind {
            Kind::Tool(_) => return Err(AgentSessionError::Failed),
            Kind::Plan => {
                if response
                    .get("updatedInput")
                    .is_some_and(|input| input != &self.request["input"])
                {
                    return Err(AgentSessionError::Rejected);
                }
                if allow {
                    json!({"outcome":"accepted"})
                } else {
                    let mut outcome = json!({"outcome":"rejected"});
                    if let Some(reason) = response["message"].as_str() {
                        outcome["reason"] = json!(reason);
                    }
                    outcome
                }
            }
            Kind::Question(questions) if allow => {
                if response["updatedInput"].as_object().is_none_or(|input| {
                    input.iter().any(|(key, value)| {
                        key != "answers" && self.request["input"].get(key) != Some(value)
                    })
                }) {
                    return Err(AgentSessionError::Rejected);
                }
                json!({"outcome":"answered","answers":answers(questions, response)?})
            }
            Kind::Question(_) => json!({"outcome":"skipped","reason":"User declined"}),
        };
        Ok(json!({"jsonrpc":"2.0","id":self.rpc_id,"result":{"outcome":outcome}}))
    }
}

fn answers(questions: &[Value], response: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let supplied = response["updatedInput"]["answers"]
        .as_object()
        .filter(|answers| answers.len() == questions.len())
        .ok_or(AgentSessionError::Rejected)?;
    questions
        .iter()
        .map(|question| {
            let id = text(question, "id")?;
            let value = supplied.get(id).ok_or(AgentSessionError::Rejected)?;
            let selections = if let Some(label) = value.as_str() {
                vec![label]
            } else {
                value
                    .as_array()
                    .ok_or(AgentSessionError::Rejected)?
                    .iter()
                    .map(|value| value.as_str().ok_or(AgentSessionError::Rejected))
                    .collect::<Result<Vec<_>, _>>()?
            };
            if selections.is_empty() || (question["allowMultiple"] != true && selections.len() != 1)
            {
                return Err(AgentSessionError::Rejected);
            }
            let options = question["options"]
                .as_array()
                .ok_or(AgentSessionError::Failed)?;
            let mut unique = BTreeSet::new();
            let selected = selections
                .into_iter()
                .map(|label| {
                    if !unique.insert(label) {
                        return Err(AgentSessionError::Rejected);
                    }
                    options
                        .iter()
                        .find(|option| option["label"] == label)
                        .map(|option| option["id"].clone())
                        .ok_or(AgentSessionError::Rejected)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({"questionId":id,"selectedOptionIds":selected}))
        })
        .collect()
}

#[cfg(test)]
mod tests;
