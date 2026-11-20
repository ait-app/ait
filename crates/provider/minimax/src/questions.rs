//! Native questionnaires shown as one host question per step.
//!
//! `MiniMax` Code sends every `ask_user` step as a form property and, for a step with choices
//! that also takes a free-form answer, a second `<step>__other` text property. The shared form
//! projection lists properties by key, so a questionnaire would show each step twice, in key
//! order and under its property key. This module folds the pair back into one question with the
//! native order, header and choice descriptions, and answers both properties from it.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use provider_core::ports::agent_session::AgentSessionError;
use provider_native::acp_permissions::Pending;

/// Choices of a normalized form property: its items for a multiple choice, else itself.
fn choices(property: &Value) -> &Value {
    if property["type"] == "array" {
        &property["items"]
    } else {
        property
    }
}

/// Whether every property has a shape `ask_user` emits: one choice, several choices, or text.
///
/// Other forms keep the shared projection, whose answers are coerced to the property type.
fn questionnaire(properties: &Map<String, Value>) -> bool {
    properties
        .values()
        .all(|property| match property["type"].as_str() {
            Some("string") => true,
            Some("array") => property["items"].get("enum").is_some(),
            _ => false,
        })
}

/// Key of the free-form text property that `MiniMax` Code pairs with the choice step `key`.
fn other_key<'a>(properties: &'a Map<String, Value>, key: &str, title: &str) -> Option<&'a str> {
    let prefix = format!("{key}__other");
    let label = format!("{title} \u{2014} Other");
    properties
        .iter()
        .find(|(name, property)| {
            name.strip_prefix(&prefix)
                .is_some_and(|rest| rest.chars().all(|character| character == '_'))
                && property["type"] == "string"
                && property.get("enum").is_none()
                && property["title"] == label.as_str()
        })
        .map(|(name, _)| name.as_str())
}

/// Host options of one step: native titles with the descriptions of the raw choice list.
fn options(property: &Value, raw: &Value) -> Vec<Value> {
    let listed = choices(property);
    let raw = choices(raw);
    let described = raw
        .get("oneOf")
        .or_else(|| raw.get("anyOf"))
        .and_then(Value::as_array);
    listed["enumNames"]
        .as_array()
        .or_else(|| listed["enum"].as_array())
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, label)| {
            let mut option = json!({"label":label});
            if let Some(description) = described
                .and_then(|described| described.get(index))
                .and_then(|choice| choice["description"].as_str())
            {
                option["description"] = json!(description);
            }
            option
        })
        .collect()
}

/// Replace the host questions of an admitted native form with one question per step.
///
/// A form with a property that `ask_user` never emits is left as the shared projection built it.
///
/// # Arguments
///
/// * `pending` - Form admitted by the shared projection; its request is rewritten in place.
/// * `message` - Native `elicitation/create` callback, read for choice descriptions.
/// * `asked` - Latest `ask_user` tool update, whose steps give the native order and headers.
pub(super) fn project(pending: &mut Pending, message: &Value, asked: Option<&Value>) {
    let Some(properties) = pending
        .schema
        .as_ref()
        .and_then(|schema| schema["properties"].as_object())
    else {
        return;
    };
    if !questionnaire(properties) {
        return;
    }
    let raw = &message["params"]["requestedSchema"];
    let required = |key: &str| {
        raw["required"]
            .as_array()
            .is_some_and(|required| required.iter().any(|name| name == key))
    };
    let steps = asked
        .and_then(|asked| asked["rawInput"]["steps"].as_array())
        .map_or(&[][..], Vec::as_slice);
    let mut folded = BTreeSet::new();
    let mut questions = Vec::with_capacity(properties.len());
    for (key, property) in properties {
        let title = property["title"].as_str().unwrap_or(key);
        let chosen = choices(property).get("enum").is_some();
        // A required choice must itself be answered, so its free-form text stays a question.
        let other = (chosen && !required(key))
            .then(|| other_key(properties, key, title))
            .flatten();
        folded.extend(other);
        // The native form trims the question that the tool input carries verbatim.
        let step = steps
            .iter()
            .position(|step| step["question"].as_str().map(str::trim) == Some(title.trim()));
        // A text step without a description sends its input placeholder in that field.
        let description = property["description"]
            .as_str()
            .filter(|_| chosen || step.is_none_or(|step| steps[step]["description"].is_string()));
        let mut question = json!({
            "id":key,
            "header":step.and_then(|step| steps[step]["header"].as_str()).map(str::trim),
            "question":title,
            "description":description,
            "options":options(property, &raw["properties"][key]),
            "multiSelect":property["type"] == "array",
            "allowOther":other.is_some() || !chosen,
            // A choice step is answered by a choice or by its free-form text.
            "allowEmpty":!chosen && !required(key),
        });
        if property["type"] == "array" {
            question["answerFormat"] = json!("array");
        }
        if let Some(other) = other {
            question["otherKey"] = json!(other);
        }
        questions.push((step.unwrap_or(usize::MAX), question));
    }
    questions.retain(|(_, question)| {
        question["id"]
            .as_str()
            .is_none_or(|key| !folded.contains(key))
    });
    questions.sort_by_key(|(step, _)| *step);
    let matched = questions.iter().any(|(step, _)| *step != usize::MAX);
    let mut headers = BTreeSet::new();
    let questions = questions
        .into_iter()
        .map(|(_, mut question)| {
            // Answers come back under the header, so it must name exactly one question.
            let unique = question["header"].as_str().is_some_and(|header| {
                !header.is_empty()
                    && !properties.contains_key(header)
                    && headers.insert(header.to_owned())
            });
            if !unique {
                question["header"] = question["id"].clone();
            }
            question
        })
        .collect::<Vec<_>>();
    pending.request["input"]["questions"] = json!(questions);
    if matched && let Some(call) = asked.and_then(|asked| asked.get("toolCallId")) {
        pending.request["metadata"]["toolCallId"] = call.clone();
    }
}

/// Native value of the choice the host shows as `label`, if the step lists it.
fn native(listed: &Value, label: &str) -> Option<Value> {
    let position = listed["enumNames"]
        .as_array()
        .or_else(|| listed["enum"].as_array())?
        .iter()
        .position(|name| name == label)?;
    listed["enum"].get(position).cloned()
}

/// Add the native content of one answered question to `content`.
fn fill(
    content: &mut Map<String, Value>,
    question: &Value,
    property: &Value,
    answer: &Value,
) -> Result<(), AgentSessionError> {
    let key = question["id"].as_str().ok_or(AgentSessionError::Failed)?;
    let listed = choices(property);
    let texts = match answer {
        Value::String(text) => vec![text.as_str()],
        Value::Array(items) if property["type"] == "array" => items
            .iter()
            .map(|item| item.as_str().ok_or(AgentSessionError::Rejected))
            .collect::<Result<_, _>>()?,
        _ => return Err(AgentSessionError::Rejected),
    };
    let mut selected = Vec::new();
    let mut written = Vec::new();
    for text in texts.into_iter().filter(|text| !text.trim().is_empty()) {
        match native(listed, text) {
            Some(value) => selected.push(value),
            None => written.push(text),
        }
    }
    if listed.get("enum").is_none() {
        if let Some(text) = written.first() {
            content.insert(key.to_owned(), json!(text));
        }
        return Ok(());
    }
    if property["type"] == "array" {
        if !selected.is_empty() {
            content.insert(key.to_owned(), json!(selected));
        }
    } else if let Some(value) = selected.into_iter().next() {
        content.insert(key.to_owned(), value);
    }
    if !written.is_empty() {
        let other = question["otherKey"]
            .as_str()
            .ok_or(AgentSessionError::Rejected)?;
        content.insert(other.to_owned(), json!(written.join(", ")));
    }
    Ok(())
}

/// Convert a host answer to a projected questionnaire into native form content.
///
/// # Arguments
///
/// * `pending` - Callback whose request was rewritten by [`project`].
/// * `response` - Host answer; question-UI `answers` are keyed by question header or identity.
///
/// # Returns
/// The answer with native `content` for the shared form resolver, or `None` when `response`
/// needs no conversion: a tool permission, a refusal, an answer that already has `content`, or a
/// form that [`project`] left to the shared projection.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for an answer to an unknown question, a value of the
/// wrong shape, or free-form text for a step that takes none, and `AgentSessionError::Failed`
/// for a retained request without question identities.
pub(super) fn answer(
    pending: &Pending,
    response: &Value,
) -> Result<Option<Value>, AgentSessionError> {
    let (Some(schema), Some(answers)) = (
        &pending.schema,
        response["updatedInput"]["answers"].as_object(),
    ) else {
        return Ok(None);
    };
    if response["behavior"] != "allow"
        || response["updatedInput"].get("content").is_some()
        || !schema["properties"].as_object().is_some_and(questionnaire)
    {
        return Ok(None);
    }
    let questions = pending.request["input"]["questions"]
        .as_array()
        .ok_or(AgentSessionError::Failed)?;
    let mut content = Map::new();
    let mut answered = 0;
    for question in questions {
        let Some(answer) = ["header", "id"]
            .iter()
            .filter_map(|field| question[*field].as_str())
            .find_map(|name| answers.get(name))
        else {
            continue;
        };
        answered += 1;
        let key = question["id"].as_str().ok_or(AgentSessionError::Failed)?;
        fill(&mut content, question, &schema["properties"][key], answer)?;
    }
    // Every submitted answer must belong to a question; a header never equals another identity.
    if answered != answers.len() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(Some(
        json!({"behavior":"allow","updatedInput":{"content":content}}),
    ))
}

#[cfg(test)]
mod tests;
