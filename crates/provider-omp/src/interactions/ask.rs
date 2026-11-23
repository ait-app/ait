//! OMP's `ask` tool (D15). OMP 18.18.8 asks one form field at a time: a choice field is a
//! `select` offering `Other (type your own)` (a multi-choice field re-prompts `(n selected)`
//! until `… Done selecting`), and typed text follows as an `input`. Ait shows each choice
//! field as one question card and replays the answer through OMP's follow-up requests, as
//! Paseo's `question-ui.ts` does.

use std::collections::VecDeque;

use serde_json::{Map, Value, json};

use crate::PROVIDER;
use provider::ports::agent_session::AgentSessionError;

/// Option OMP adds to a choice field for a typed answer.
const OTHER: &str = "Other (type your own)";
/// Suffix of the option ending a multi-choice field; its mark depends on OMP's theme.
const DONE: &str = " Done selecting";
/// Suffix OMP adds to a recommended option of a pre-18.18 `ask` question.
const RECOMMENDED: &str = " (Recommended)";
/// Bound on one answer value, matching other dialog answers.
const MAX_ANSWER: usize = 65536;

/// Questions of the running `ask` call and the answer still being replayed.
#[derive(Debug, Default)]
pub(super) struct Ask {
    /// Tool call identity of the running `ask`; its choice selects become question cards.
    call: Option<String>,
    /// Questions from a pre-18.18 `questions` argument, for option descriptions and multi.
    questions: Vec<Question>,
    pending: Option<Replay>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Question {
    title: String,
    multi: bool,
    /// Option labels with their descriptions.
    descriptions: Vec<(String, String)>,
}

/// One answered card's remaining replies to OMP's follow-up requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Replay {
    /// Field title without the `(n selected)` prefix.
    title: String,
    /// A single-choice card replays only a re-prompt revealing a multi-choice field.
    multi: bool,
    selected: VecDeque<String>,
    custom: Option<String>,
    next: Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Next {
    Select,
    Text,
}

impl Ask {
    /// Track the `ask` call from the assistant's tool call, `tool_execution_start` and
    /// `tool_execution_end`. OMP 18.18.8 sends the first `select` before
    /// `tool_execution_start`, so the assistant message is the first signal.
    pub(super) fn observe(&mut self, event: &Value) {
        match event["type"].as_str() {
            Some("message_end") if event["message"]["role"] == "assistant" => {
                for item in event["message"]["content"].as_array().into_iter().flatten() {
                    if item["type"] == "toolCall" && item["name"] == "ask" {
                        self.start(&item["id"], &item["arguments"]);
                    }
                }
            }
            Some("tool_execution_start") if event["toolName"] == "ask" => {
                self.start(&event["toolCallId"], &event["args"]);
            }
            Some("tool_execution_end") if event["toolName"] == "ask" => *self = Self::default(),
            _ => {}
        }
    }

    fn start(&mut self, call: &Value, arguments: &Value) {
        let call = call.as_str().unwrap_or_default();
        if self.call.as_deref() != Some(call) {
            *self = Self {
                call: Some(call.to_owned()),
                questions: questions(arguments),
                pending: None,
            };
        }
    }

    /// Map an `ask` choice `select` to one question card.
    ///
    /// # Returns
    ///
    /// The card, or `None` outside an `ask` call or for a select without `Other` or `Done`.
    pub(super) fn card(&self, native: &Value, id: &str) -> Option<Value> {
        self.call.as_ref()?;
        let options = native["options"].as_array()?;
        let labels = options
            .iter()
            .map(Value::as_str)
            .collect::<Option<Vec<_>>>()?;
        let done = labels.iter().any(|label| label.ends_with(DONE));
        let other = labels.contains(&OTHER);
        if !done && !other {
            return None;
        }
        let (selecting, title) = selected_count(native["title"].as_str()?);
        let question = self.question(title);
        let multi = done || selecting || question.is_some_and(|question| question.multi);
        let choices: Vec<Value> = labels
            .iter()
            .enumerate()
            .filter(|(_, label)| **label != OTHER && !label.ends_with(DONE))
            .map(|(index, label)| {
                let mut choice = json!({"label":label});
                let description = question
                    .and_then(|question| question.description(label))
                    .or_else(|| native["optionDetails"][index]["description"].as_str())
                    .filter(|description| !description.trim().is_empty());
                if let Some(description) = description {
                    choice["description"] = json!(description);
                }
                choice
            })
            .collect();
        let shown = question.map_or(title, |question| question.title.as_str());
        let mut card = json!({"header":shown,"question":shown,"answerKey":"value",
            "options":choices,"multiSelect":multi,"allowOther":other});
        if multi {
            card["answerFormat"] = json!("array");
        }
        Some(
            json!({"id":id,"provider":PROVIDER,"name":"ask","kind":"question","title":shown,
            "input":{"questions":[card]},"actions":super::actions(),
            "metadata":{"extensionUiMethod":"select","ompAsk":true}}),
        )
    }

    fn question(&self, title: &str) -> Option<&Question> {
        self.questions.iter().find(|question| {
            title == question.title
                || title
                    .strip_prefix(question.title.as_str())
                    .and_then(|rest| rest.strip_prefix(" ("))
                    .and_then(|rest| rest.strip_suffix(')'))
                    .and_then(|rest| rest.split_once('/'))
                    .is_some_and(|(index, total)| is_number(index) && is_number(total))
        })
    }

    /// Answer a follow-up request of the replayed card without asking the host.
    ///
    /// # Returns
    ///
    /// The native reply, or `None` when `native` does not continue the replay; any other
    /// request ends it.
    pub(super) fn replay(&mut self, native: &Value) -> Option<Value> {
        let mut pending = self.pending.take()?;
        let id = native["id"].as_str()?;
        let title = native["title"].as_str();
        let mut reply = match (pending.next, native["method"].as_str()) {
            (Next::Text, Some("input" | "editor")) => {
                if native["method"] == "input"
                    && title.map(|title| selected_count(title).1) != Some(pending.title.as_str())
                {
                    return None;
                }
                let custom = pending.custom.take()?;
                pending.next = Next::Select;
                self.pending = Some(pending);
                json!({"value":custom})
            }
            (Next::Select, Some("select")) => {
                let (selecting, title) = selected_count(title?);
                if title != pending.title || (!pending.multi && !selecting) {
                    return None;
                }
                let options: Vec<&str> = native["options"]
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .collect();
                if let Some(choice) = pending
                    .selected
                    .front()
                    .filter(|choice| options.contains(&choice.as_str()))
                {
                    let reply = json!({"value":choice});
                    pending.selected.pop_front();
                    self.pending = Some(pending);
                    reply
                } else if pending.custom.is_some() && options.contains(&OTHER) {
                    pending.next = Next::Text;
                    self.pending = Some(pending);
                    json!({"value":OTHER})
                } else if let Some(done) = options.iter().find(|option| option.ends_with(DONE)) {
                    json!({"value":done})
                } else {
                    // OMP omits Done when a field still needs an answer; nothing can follow.
                    json!({"cancelled":true})
                }
            }
            _ => return None,
        };
        reply["type"] = json!("extension_ui_response");
        reply["id"] = json!(id);
        Some(reply)
    }

    /// Start replaying an answered card, or end the replay with `None`.
    pub(super) fn begin(&mut self, replay: Option<Replay>) {
        self.pending = replay;
    }
}

impl Question {
    fn description(&self, label: &str) -> Option<&str> {
        let plain = label.strip_suffix(RECOMMENDED).unwrap_or(label);
        self.descriptions
            .iter()
            .find(|(option, _)| option == label || option == plain)
            .map(|(_, description)| description.as_str())
    }
}

/// Build the native reply to a question card and the replay of its follow-up requests.
///
/// # Arguments
///
/// * `id` - Native request identity.
/// * `native` - The native `select`.
/// * `card` - Host request built by [`Ask::card`].
/// * `answers` - The host's answers object.
///
/// # Returns
///
/// The first native reply and the replay to begin. An empty answer cancels OMP's `ask`.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for a missing or malformed answer, duplicate
/// choices, several typed answers, typed text OMP does not offer, or several answers to a
/// single-choice card.
pub(super) fn answer(
    id: &str,
    native: &Value,
    card: &Value,
    answers: &Map<String, Value>,
) -> Result<(Value, Option<Replay>), AgentSessionError> {
    let question = &card["input"]["questions"][0];
    let multi = question["multiSelect"] == true;
    let labels: Vec<&str> = question["options"]
        .as_array()
        .ok_or(AgentSessionError::Rejected)?
        .iter()
        .filter_map(|option| option["label"].as_str())
        .collect();
    let value = answers
        .get("value")
        .or_else(|| answers.get(question["header"].as_str().unwrap_or_default()))
        .ok_or(AgentSessionError::Rejected)?;
    let (selected, custom) = match value {
        Value::String(text) if multi => split_multi_select_answer(text, &labels),
        Value::String(text) => classify(&[text.as_str()], &labels)?,
        Value::Array(values) => classify(
            &values
                .iter()
                .map(Value::as_str)
                .collect::<Option<Vec<_>>>()
                .ok_or(AgentSessionError::Rejected)?,
            &labels,
        )?,
        _ => return Err(AgentSessionError::Rejected),
    };
    if (!multi && selected.len() + usize::from(custom.is_some()) > 1)
        || (custom.is_some() && question["allowOther"] != true)
    {
        return Err(AgentSessionError::Rejected);
    }
    if custom.as_ref().is_some_and(|text| {
        text.trim().is_empty() || text.len() > MAX_ANSWER || text.contains('\0')
    }) {
        return Err(AgentSessionError::Rejected);
    }
    let mut selected: VecDeque<String> = selected.into_iter().map(str::to_owned).collect();
    let (first, next) = match selected.pop_front() {
        Some(choice) => (choice, Next::Select),
        None if custom.is_some() => (OTHER.to_owned(), Next::Text),
        None => {
            return Ok((
                json!({"type":"extension_ui_response","id":id,"cancelled":true}),
                None,
            ));
        }
    };
    let replay = Replay {
        title: native["title"]
            .as_str()
            .map(|title| selected_count(title).1.to_owned())
            .unwrap_or_default(),
        multi,
        selected,
        custom,
        next,
    };
    Ok((
        json!({"type":"extension_ui_response","id":id,"value":first}),
        Some(replay),
    ))
}

/// Split answer values into exact option labels and at most one typed answer.
fn classify<'a>(
    values: &[&'a str],
    labels: &[&str],
) -> Result<(Vec<&'a str>, Option<String>), AgentSessionError> {
    let mut selected: Vec<&str> = Vec::with_capacity(values.len());
    let mut custom = None;
    for value in values {
        if labels.contains(value) {
            if selected.contains(value) {
                return Err(AgentSessionError::Rejected);
            }
            selected.push(value);
        } else if custom.replace((*value).to_owned()).is_some() {
            return Err(AgentSessionError::Rejected);
        }
    }
    Ok((selected, custom))
}

/// Split a multi-select answer sent as one string, as Paseo's `question-card-answer.ts`
/// does: exact option labels joined with `", "` from the front, longest label first so a
/// label containing `", "` wins over its prefix; any remaining text is the typed answer.
///
/// # Arguments
///
/// * `answer` - The joined answer.
/// * `labels` - The card's option labels.
///
/// # Returns
///
/// The labels in answer order, and the remaining typed text, if any.
pub(super) fn split_multi_select_answer<'a>(
    answer: &str,
    labels: &[&'a str],
) -> (Vec<&'a str>, Option<String>) {
    let mut ordered = labels.to_vec();
    ordered.sort_by_key(|label| std::cmp::Reverse(label.len()));
    let mut remaining = answer;
    let mut selected = Vec::new();
    while !remaining.is_empty() {
        let Some(label) = ordered.iter().copied().find(|label| {
            !selected.contains(label)
                && (remaining == *label
                    || remaining
                        .strip_prefix(*label)
                        .is_some_and(|rest| rest.starts_with(", ")))
        }) else {
            break;
        };
        selected.push(label);
        remaining = remaining.get(label.len() + 2..).unwrap_or_default();
    }
    (
        selected,
        Some(remaining.to_owned()).filter(|rest| !rest.is_empty()),
    )
}

/// Split OMP's `(n selected) ` prefix from a multi-choice title; `true` when it was there.
fn selected_count(title: &str) -> (bool, &str) {
    title
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(" selected) "))
        .filter(|(count, _)| is_number(count))
        .map_or((false, title), |(_, rest)| (true, rest))
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Questions of a pre-18.18 `ask` call's `questions` argument; OMP 18.18.8 sends an HTML form.
fn questions(arguments: &Value) -> Vec<Question> {
    arguments["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|question| {
            Some(Question {
                title: question["question"].as_str()?.to_owned(),
                multi: question["multi"] == true,
                descriptions: question["options"]
                    .as_array()?
                    .iter()
                    .filter_map(|option| {
                        Some((
                            option["label"].as_str()?.to_owned(),
                            option["description"].as_str()?.to_owned(),
                        ))
                    })
                    .collect(),
            })
        })
        .collect()
}
