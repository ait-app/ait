use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::ports::agent_session::{AgentSessionError, AgentTurnEvent};

#[derive(Debug)]
struct Dialog {
    native: Value,
    request: Value,
    expires: Option<Instant>,
}

#[derive(Debug, Default)]
pub(super) struct Dialogs(BTreeMap<String, Dialog>);

impl Dialogs {
    pub(super) fn capture(
        &mut self,
        native: &Value,
    ) -> Result<Vec<AgentTurnEvent>, AgentSessionError> {
        if native["method"] == "cancel" {
            return Ok(self
                .0
                .remove(native["targetId"].as_str().unwrap_or_default())
                .map(|dialog| {
                    AgentTurnEvent::PermissionResolved(
                        dialog.request["id"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .into_iter()
                .collect());
        }
        let method = native["method"].as_str().ok_or(AgentSessionError::Failed)?;
        if !matches!(method, "select" | "confirm" | "input" | "editor" | "ask") {
            return Ok(Vec::new());
        }
        let id = super::config::text(native, "id")?.to_owned();
        if self.0.len() >= 32 || self.0.contains_key(&id) || native.to_string().len() > 65536 {
            return Err(AgentSessionError::Failed);
        }
        let title = native["title"].as_str().unwrap_or("OMP question");
        let options = if method == "select" {
            native["options"]
                .as_array()
                .filter(|options| options.len() <= 128)
                .ok_or(AgentSessionError::Failed)?
                .iter()
                .map(|option| {
                    Ok(json!({"label":option.as_str().ok_or(AgentSessionError::Failed)?}))
                })
                .collect::<Result<Vec<_>, AgentSessionError>>()?
        } else {
            Vec::new()
        };
        let mut request = json!({"id":id,"provider":"omp","name":"extension_ui","kind":
            if method=="confirm" {"tool"} else {"question"},"description":native["message"],
            "input":if method=="confirm" {json!({"title":title,"message":native["message"]})}
            else {json!({"questions":[{"header":title,"question":title,"answerKey":"value",
                "options":options,"multiSelect":false,"allowOther":method!="select"}]})},
            "actions":[{"id":"allow","label":"Submit","behavior":"allow"},
                {"id":"deny","label":"Cancel","behavior":"deny"}]});
        if method == "ask" {
            request["input"] = json!({"questions":questions(native)?});
        }
        let expires = native["timeout"].as_u64().map(|milliseconds| {
            Instant::now() + Duration::from_millis(milliseconds.min(86_400_000))
        });
        self.0.insert(
            id,
            Dialog {
                native: native.clone(),
                request: request.clone(),
                expires,
            },
        );
        Ok(vec![AgentTurnEvent::PermissionRequested(request)])
    }

    pub(super) fn pending(&self) -> Vec<Value> {
        self.0
            .values()
            .filter(|dialog| dialog.expires.is_none_or(|end| end > Instant::now()))
            .map(|dialog| dialog.request.clone())
            .collect()
    }

    pub(super) fn response(&self, id: &str, response: &Value) -> Result<Value, AgentSessionError> {
        let dialog = self
            .0
            .get(id)
            .filter(|dialog| dialog.expires.is_none_or(|end| end > Instant::now()))
            .ok_or(AgentSessionError::Rejected)?;
        let allow = match response["behavior"].as_str() {
            Some("allow") => true,
            Some("deny") => false,
            _ => return Err(AgentSessionError::Rejected),
        };
        if response
            .get("selectedActionId")
            .is_some_and(|action| action != if allow { "allow" } else { "deny" })
        {
            return Err(AgentSessionError::Rejected);
        }
        if !allow {
            return Ok(json!({"type":"extension_ui_response","id":id,"cancelled":true}));
        }
        if dialog.native["method"] == "ask" {
            return answer_questions(id, &dialog.native, response);
        }
        if dialog.native["method"] == "confirm" {
            if response
                .get("updatedInput")
                .is_some_and(|input| input != &dialog.request["input"])
            {
                return Err(AgentSessionError::Rejected);
            }
            return Ok(json!({"type":"extension_ui_response","id":id,"confirmed":true}));
        }
        let answers = response["updatedInput"]["answers"]
            .as_object()
            .filter(|answers| answers.len() == 1)
            .ok_or(AgentSessionError::Rejected)?;
        let value = answers
            .get("value")
            .or_else(|| answers.get(dialog.native["title"].as_str().unwrap_or("OMP question")))
            .and_then(Value::as_str)
            .filter(|value| value.len() <= 65536 && !value.contains('\0'))
            .ok_or(AgentSessionError::Rejected)?;
        if dialog.native["method"] == "select"
            && !dialog.native["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option == value))
        {
            return Err(AgentSessionError::Rejected);
        }
        Ok(json!({"type":"extension_ui_response","id":id,"value":value}))
    }

    pub(super) fn remove(&mut self, id: &str) {
        self.0.remove(id);
    }

    pub(super) fn clear(&mut self) -> Vec<AgentTurnEvent> {
        std::mem::take(&mut self.0)
            .into_keys()
            .map(AgentTurnEvent::PermissionResolved)
            .collect()
    }
}

#[cfg(test)]
mod tests;

fn questions(native: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let questions = native["questions"]
        .as_array()
        .filter(|questions| !questions.is_empty() && questions.len() <= 32)
        .ok_or(AgentSessionError::Failed)?;
    let mut ids = std::collections::BTreeSet::new();
    questions
        .iter()
        .map(|question| {
            let id = super::config::text(question, "id")?;
            if !ids.insert(id) {
                return Err(AgentSessionError::Failed);
            }
            let options = question["options"]
                .as_array()
                .filter(|options| options.len() <= 128)
                .ok_or(AgentSessionError::Failed)?;
            let mut labels = std::collections::BTreeSet::new();
            for option in options {
                if !labels.insert(super::config::text(option, "label")?) {
                    return Err(AgentSessionError::Failed);
                }
            }
            Ok(
                json!({"answerKey":id,"header":question["header"].as_str().unwrap_or(id),
            "question":super::config::text(question,"question")?,"options":options,
            "multiSelect":question["multi"]==true,"allowOther":true,"answerFormat":"array"}),
            )
        })
        .collect()
}

fn answer_questions(
    id: &str,
    native: &Value,
    response: &Value,
) -> Result<Value, AgentSessionError> {
    let questions = questions(native)?;
    let supplied = response["updatedInput"]["answers"]
        .as_object()
        .filter(|answers| answers.len() == questions.len())
        .ok_or(AgentSessionError::Rejected)?;
    let mut answers = Vec::with_capacity(questions.len());
    for question in &questions {
        let key = super::config::text(question, "answerKey")?;
        let values = supplied
            .get(key)
            .and_then(Value::as_array)
            .filter(|values| values.len() <= 128)
            .ok_or(AgentSessionError::Rejected)?;
        if question["multiSelect"] != true && values.len() != 1 {
            return Err(AgentSessionError::Rejected);
        }
        let mut selected = Vec::new();
        let mut custom = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for value in values {
            let text = value
                .as_str()
                .filter(|text| {
                    !text.trim().is_empty() && text.len() <= 4096 && !text.contains('\0')
                })
                .ok_or(AgentSessionError::Rejected)?;
            if !seen.insert(text) {
                return Err(AgentSessionError::Rejected);
            }
            if question["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["label"] == text))
            {
                selected.push(text);
            } else {
                custom.push(text);
            }
        }
        if custom.len() > 1 {
            return Err(AgentSessionError::Rejected);
        }
        let mut answer = json!({"id":key,"selectedOptions":selected});
        if let Some(text) = custom.first() {
            answer["customInput"] = json!(text);
        }
        answers.push(answer);
    }
    Ok(json!({"type":"extension_ui_response","id":id,"answers":answers}))
}
