//! OMP `extension_ui_request` dialogs: tool approvals (`approval`), `ask` choice fields
//! (`ask`), structured `ask` questions, and generic `select`, `confirm`, `input` and
//! `editor` questions (D13). Every card keeps the ADR-198 client contract.

mod approval;
mod ask;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::PROVIDER;
use provider::ports::agent_session::{AgentSessionError, AgentTurnEvent};

/// Bound on one answer value.
const MAX_ANSWER: usize = 65536;
/// Bound on one published question; a tool approval bounds its detail instead.
const MAX_REQUEST: usize = 65536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Generic,
    Approval,
    Ask,
    Structured,
}

#[derive(Debug)]
struct Dialog {
    kind: Kind,
    native: Value,
    request: Value,
    expires: Option<Instant>,
}

/// What a native `extension_ui_request` needs from the session.
#[derive(Debug, PartialEq)]
pub(super) enum Captured {
    /// Not a dialog, such as `notify` or `open_url`; the stream may still show it.
    Ignored,
    /// Host events: a published request, or a resolution after OMP cancelled one.
    Events(Vec<AgentTurnEvent>),
    /// A follow-up of an answered `ask` card, answered without the host.
    Reply(Value),
    /// A question too large to publish, cancelled with this reply; the turn continues.
    Declined(Value),
}

/// A validated host answer: the native reply, and the `ask` replay it starts once sent.
#[derive(Debug)]
pub(super) struct Answer {
    pub(super) native: Value,
    replay: Option<ask::Replay>,
}

impl Answer {
    const fn reply(native: Value) -> Self {
        Self {
            native,
            replay: None,
        }
    }
}

/// Pending native dialogs and the running `ask` call.
#[derive(Debug, Default)]
pub(super) struct Dialogs {
    open: BTreeMap<String, Dialog>,
    ask: ask::Ask,
}

impl Dialogs {
    /// Track the `ask` tool call from session events.
    pub(super) fn observe(&mut self, event: &Value) {
        self.ask.observe(event);
    }

    /// Classify one native `extension_ui_request`.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Failed` for a dialog without a method or a valid identity,
    /// beyond 32 open dialogs, for a repeated identity or malformed options or questions. A
    /// question over 64 KiB is declined; a tool approval shows bounded detail instead.
    pub(super) fn capture(&mut self, native: &Value) -> Result<Captured, AgentSessionError> {
        if native["method"] == "cancel" {
            return Ok(Captured::Events(
                self.open
                    .remove(native["targetId"].as_str().unwrap_or_default())
                    .map(|dialog| {
                        AgentTurnEvent::PermissionResolved(
                            dialog.request["id"].as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .into_iter()
                    .collect(),
            ));
        }
        let method = native["method"].as_str().ok_or(AgentSessionError::Failed)?;
        if !matches!(method, "select" | "confirm" | "input" | "editor" | "ask") {
            return Ok(Captured::Ignored);
        }
        if let Some(reply) = self.ask.replay(native) {
            return Ok(Captured::Reply(reply));
        }
        let id = super::config::text(native, "id")?.to_owned();
        if self.open.len() >= 32 || self.open.contains_key(&id) {
            return Err(AgentSessionError::Failed);
        }
        // Only the id and the chosen option label answer an approval (D14).
        let approval = (method == "select")
            .then(|| approval::request(native, &id))
            .flatten();
        let (kind, request) = match approval {
            Some(request) => (Kind::Approval, request),
            None if native.to_string().len() > MAX_REQUEST => {
                tracing::debug!(
                    provider = super::LABEL,
                    "Cancelling an oversized OMP question"
                );
                return Ok(Captured::Declined(
                    json!({"type":"extension_ui_response","id":id,"cancelled":true}),
                ));
            }
            None => match method {
                "ask" => (Kind::Structured, structured(native, &id)?),
                "select" => self.select(native, &id)?,
                _ => (Kind::Generic, generic(native, &id, method)?),
            },
        };
        let expires = native["timeout"].as_u64().map(|milliseconds| {
            Instant::now() + Duration::from_millis(milliseconds.min(86_400_000))
        });
        self.open.insert(
            id,
            Dialog {
                kind,
                // An approval is answered from its request alone.
                native: if kind == Kind::Approval {
                    Value::Null
                } else {
                    native.clone()
                },
                request: request.clone(),
                expires,
            },
        );
        Ok(Captured::Events(vec![AgentTurnEvent::PermissionRequested(
            request,
        )]))
    }

    /// An `ask` choice field (D15), then a generic question; tool approvals (D14) are
    /// recognised first by [`Dialogs::capture`].
    fn select(&self, native: &Value, id: &str) -> Result<(Kind, Value), AgentSessionError> {
        if let Some(request) = self.ask.card(native, id) {
            return Ok((Kind::Ask, request));
        }
        Ok((Kind::Generic, generic(native, id, "select")?))
    }

    pub(super) fn pending(&self) -> Vec<Value> {
        self.open
            .values()
            .filter(|dialog| dialog.expires.is_none_or(|end| end > Instant::now()))
            .map(|dialog| dialog.request.clone())
            .collect()
    }

    /// Validate a host answer and build its native reply.
    ///
    /// # Errors
    ///
    /// Returns `AgentSessionError::Rejected` for an unknown or expired dialog, an unknown
    /// behavior or action, or an answer the dialog does not accept.
    pub(super) fn response(&self, id: &str, response: &Value) -> Result<Answer, AgentSessionError> {
        let dialog = self
            .open
            .get(id)
            .filter(|dialog| dialog.expires.is_none_or(|end| end > Instant::now()))
            .ok_or(AgentSessionError::Rejected)?;
        if dialog.kind == Kind::Approval {
            return Ok(Answer::reply(approval::reply(
                id,
                &dialog.request,
                response,
            )?));
        }
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
            return Ok(Answer::reply(
                json!({"type":"extension_ui_response","id":id,"cancelled":true}),
            ));
        }
        let answers = response["updatedInput"]["answers"]
            .as_object()
            .ok_or(AgentSessionError::Rejected)?;
        match dialog.kind {
            Kind::Structured => Ok(Answer::reply(answer_questions(
                id,
                &dialog.native,
                answers,
            )?)),
            Kind::Ask => {
                let (native, replay) = ask::answer(id, &dialog.native, &dialog.request, answers)?;
                Ok(Answer { native, replay })
            }
            Kind::Generic | Kind::Approval => {
                Ok(Answer::reply(generic_answer(id, dialog, answers)?))
            }
        }
    }

    /// Forget a dialog whose `answer` was sent, and start the replay the answer needs.
    pub(super) fn resolve(&mut self, id: &str, answer: Answer) {
        if self
            .open
            .remove(id)
            .is_some_and(|dialog| dialog.kind == Kind::Ask)
        {
            self.ask.begin(answer.replay);
        }
    }

    /// Resolve every open dialog and forget the running `ask` call.
    pub(super) fn clear(&mut self) -> Vec<AgentTurnEvent> {
        self.ask = ask::Ask::default();
        std::mem::take(&mut self.open)
            .into_keys()
            .map(AgentTurnEvent::PermissionResolved)
            .collect()
    }
}

/// Host actions of every question card, as ADR-198 publishes them.
fn actions() -> Value {
    json!([{"id":"allow","label":"Submit","behavior":"allow"},
        {"id":"deny","label":"Cancel","behavior":"deny"}])
}

/// Map `select`, `confirm`, `input` and `editor` to one question answered under
/// `answerKey: "value"` or its header, as Paseo's generic questions are (D13).
fn generic(native: &Value, id: &str, method: &str) -> Result<Value, AgentSessionError> {
    let title = native["title"].as_str();
    let message = native["message"].as_str();
    let mut question = json!({"answerKey":"value","options":[],"multiSelect":false,
        "allowOther":true});
    let shown = match method {
        "select" => {
            question["options"] = json!(select_options(native)?);
            question["allowOther"] = json!(false);
            title.unwrap_or("Select an option").to_owned()
        }
        "confirm" => {
            question["options"] = json!([{"label":"Yes"},{"label":"No"}]);
            question["allowOther"] = json!(false);
            question["question"] = json!(
                [title, message]
                    .into_iter()
                    .flatten()
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n\n")
            );
            title.unwrap_or("Confirm").to_owned()
        }
        "editor" => {
            question["multiline"] = json!(true);
            title.unwrap_or("Edit text").to_owned()
        }
        _ => input(&mut question, title, native["placeholder"].as_str()),
    };
    question["header"] = json!(shown);
    if question.get("question").is_none() {
        question["question"] = json!(title.unwrap_or(&shown));
    }
    let mut request = json!({"id":id,"provider":PROVIDER,"name":"extension_ui",
        "kind":"question","input":{"questions":[question]},"actions":actions()});
    request["title"] = json!(shown);
    if let Some(message) = message {
        request["description"] = json!(message);
    }
    Ok(request)
}

/// Fill an `input` question. A placeholder mentioning "optional" or "skip" allows an empty
/// answer under a "Skip" dismiss label and titles the card "Optional comment" or "Optional
/// response", as Paseo does; the native title stays the question text.
fn input(question: &mut Value, title: Option<&str>, placeholder: Option<&str>) -> String {
    let Some(placeholder) = placeholder else {
        return title.unwrap_or("Enter a value").to_owned();
    };
    question["placeholder"] = json!(placeholder);
    if !mentions(placeholder, &["optional", "skip"]) {
        return title.unwrap_or("Enter a value").to_owned();
    }
    question["allowEmpty"] = json!(true);
    question["dismissLabel"] = json!("Skip");
    if mentions(placeholder, &["comment"])
        || title.is_some_and(|title| mentions(title, &["comment"]))
    {
        "Optional comment".to_owned()
    } else {
        "Optional response".to_owned()
    }
}

/// Whether `text` contains one of the lowercase `words` as a whole word, case-insensitively.
fn mentions(text: &str, words: &[&str]) -> bool {
    text.split(|character: char| !character.is_alphanumeric() && character != '_')
        .any(|word| words.iter().any(|wanted| word.eq_ignore_ascii_case(wanted)))
}

fn select_options(native: &Value) -> Result<Vec<Value>, AgentSessionError> {
    native["options"]
        .as_array()
        .filter(|options| options.len() <= 128)
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let mut option = json!({"label":option.as_str().ok_or(AgentSessionError::Failed)?});
            if let Some(description) = native["optionDetails"][index]["description"].as_str() {
                option["description"] = json!(description);
            }
            Ok(option)
        })
        .collect()
}

/// The native reply to a generic question: the chosen option, the typed text, or `confirmed`
/// for a Yes/No answer.
fn generic_answer(
    id: &str,
    dialog: &Dialog,
    answers: &serde_json::Map<String, Value>,
) -> Result<Value, AgentSessionError> {
    let question = &dialog.request["input"]["questions"][0];
    let value = (answers.len() == 1)
        .then(|| {
            answers
                .get("value")
                .or_else(|| answers.get(question["header"].as_str().unwrap_or_default()))
        })
        .flatten()
        .and_then(Value::as_str)
        .filter(|value| value.len() <= MAX_ANSWER && !value.contains('\0'))
        .ok_or(AgentSessionError::Rejected)?;
    match dialog.native["method"].as_str() {
        Some("confirm") => {
            let confirmed = match value.trim() {
                answer if answer.eq_ignore_ascii_case("yes") => true,
                answer if answer.eq_ignore_ascii_case("no") => false,
                _ => return Err(AgentSessionError::Rejected),
            };
            Ok(json!({"type":"extension_ui_response","id":id,"confirmed":confirmed}))
        }
        Some("select")
            if !dialog.native["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option == value)) =>
        {
            Err(AgentSessionError::Rejected)
        }
        _ => Ok(json!({"type":"extension_ui_response","id":id,"value":value})),
    }
}

/// Map OMP's structured `ask` request, sent only after `set_ask_dialog` enabled it.
fn structured(native: &Value, id: &str) -> Result<Value, AgentSessionError> {
    let mut request = json!({"id":id,"provider":PROVIDER,"name":"extension_ui",
        "kind":"question","input":{"questions":questions(native)?},"actions":actions()});
    if let Some(title) = native["title"].as_str() {
        request["title"] = json!(title);
    }
    if let Some(message) = native["message"].as_str() {
        request["description"] = json!(message);
    }
    Ok(request)
}

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
    supplied: &serde_json::Map<String, Value>,
) -> Result<Value, AgentSessionError> {
    let questions = questions(native)?;
    if supplied.len() != questions.len() {
        return Err(AgentSessionError::Rejected);
    }
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

#[cfg(test)]
mod tests;
