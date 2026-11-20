use serde_json::{Value, json};

use super::{answer, project};
use provider_core::ports::agent_session::AgentSessionError;
use provider_native::acp_permissions::{self, Pending};

/// `elicitation/create` exactly as `mcode` 0.7.7 sends it: numeric identity, no `toolCallId`.
fn form(properties: &Value, required: &[&str]) -> Value {
    let mut schema = json!({"type":"object","properties":properties});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({"jsonrpc":"2.0","id":0,"method":"elicitation/create","params":{
        "mode":"form","sessionId":"session","message":"Project setup","requestedSchema":schema}})
}

fn choice(title: &str, options: &[(&str, &str, Option<&str>)]) -> Value {
    let listed = options
        .iter()
        .map(|(id, label, description)| {
            let mut option = json!({"const":id,"title":label});
            if let Some(description) = description {
                option["description"] = json!(description);
            }
            option
        })
        .collect::<Vec<_>>();
    json!({"type":"string","title":title,"oneOf":listed})
}

fn other(title: &str) -> Value {
    json!({"type":"string","title":format!("{title} \u{2014} Other"),"description":"Others..."})
}

fn projected(message: &Value, asked: Option<&Value>) -> Pending {
    let mut pending = acp_permissions::form(message, "minimax-code").unwrap();
    project(&mut pending, message, asked);
    pending
}

fn questionnaire() -> (Value, Value) {
    let storage = "Which database should it use?";
    let language = "Which language do you prefer?";
    let mut described = choice(
        language,
        &[
            ("rust", "Rust", Some("Memory safe")),
            ("go", "Go", Some("Fast builds")),
        ],
    );
    described["description"] = json!("Used for every new service.");
    let message = form(
        &json!({
            "language":described,
            "language__other":other(language),
            "storage":choice(storage, &[("pg", "Postgres", None), ("lite", "SQLite", None)]),
            "storage__other":other(storage),
        }),
        &[],
    );
    let asked = json!({"sessionUpdate":"tool_call_update","toolCallId":"call-1","rawInput":{
        "title":"Project setup","steps":[
            {"id":"storage","header":" Storage ","question":storage},
            {"id":"language","header":"Language","question":language}]}});
    (message, asked)
}

#[test]
fn steps_fold_into_one_question_each_in_native_order() {
    let (message, asked) = questionnaire();

    let pending = projected(&message, Some(&asked));

    assert_eq!(
        pending.request["input"]["questions"],
        json!([
            {"id":"storage","header":"Storage","question":"Which database should it use?",
                "description":null,"options":[{"label":"Postgres"},{"label":"SQLite"}],
                "multiSelect":false,"allowOther":true,"allowEmpty":false,
                "otherKey":"storage__other"},
            {"id":"language","header":"Language","question":"Which language do you prefer?",
                "description":"Used for every new service.","options":[
                    {"label":"Rust","description":"Memory safe"},
                    {"label":"Go","description":"Fast builds"}],
                "multiSelect":false,"allowOther":true,"allowEmpty":false,
                "otherKey":"language__other"},
        ])
    );
    assert_eq!(pending.request["kind"], "question");
    assert_eq!(pending.request["title"], "Project setup");
    assert_eq!(pending.request["metadata"]["toolCallId"], "call-1");
}

#[test]
fn answers_return_choices_and_free_text_under_native_keys() {
    let (message, asked) = questionnaire();
    let pending = projected(&message, Some(&asked));

    let native = answer(
        &pending,
        &json!({"behavior":"allow","updatedInput":{
            "questions":pending.request["input"]["questions"],
            "answers":{"Storage":"Postgres","Language":"Zig"}}}),
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        native,
        json!({"behavior":"allow","updatedInput":{
            "content":{"storage":"pg","language__other":"Zig"}}})
    );
    assert_eq!(
        acp_permissions::resolve(&pending, &native).unwrap()["result"],
        json!({"action":"accept","content":{"storage":"pg","language__other":"Zig"}})
    );
}

#[test]
fn multiple_choices_keep_listed_values_apart_from_free_text() {
    let title = "Which tools?";
    let message = form(
        &json!({
            "tools":{"type":"array","title":title,"items":{"anyOf":[
                {"const":"rust","title":"Rust"},{"const":"nix","title":"Nix"}]}},
            "tools__other":other(title),
        }),
        &[],
    );
    let pending = projected(&message, None);
    let question = &pending.request["input"]["questions"][0];
    assert_eq!(question["header"], "tools");
    assert_eq!(question["multiSelect"], true);
    assert_eq!(question["answerFormat"], "array");
    assert_eq!(
        pending.request["input"]["questions"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let native = answer(
        &pending,
        &json!({"behavior":"allow","updatedInput":{"answers":{"tools":["Rust","Bazel","Nix"]}}}),
    )
    .unwrap()
    .unwrap();

    assert_eq!(
        native["updatedInput"]["content"],
        json!({"tools":["rust","nix"],"tools__other":"Bazel"})
    );
}

#[test]
fn forms_without_a_questionnaire_keep_their_properties_and_keys() {
    let message = form(
        &json!({
            "language":choice("Language", &[("rust", "Rust", None)]),
            "language_custom":{"type":"string","title":"Other language"},
            "notes":{"type":"string","title":"Notes"},
        }),
        &["notes"],
    );

    let pending = projected(&message, None);

    let questions = pending.request["input"]["questions"].as_array().unwrap();
    assert_eq!(
        questions
            .iter()
            .map(|question| {
                (
                    question["header"].as_str().unwrap(),
                    question["allowOther"].as_bool().unwrap(),
                    question["allowEmpty"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>(),
        [
            ("language", false, false),
            ("language_custom", true, true),
            ("notes", true, false)
        ]
    );
    assert_eq!(pending.request["metadata"]["toolCallId"], Value::Null);
    let native = answer(
        &pending,
        &json!({"behavior":"allow","updatedInput":{"answers":{
            "language":"Rust","language_custom":"","notes":"Keep it small"}}}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        native["updatedInput"]["content"],
        json!({"language":"rust","notes":"Keep it small"})
    );
}

#[test]
fn headers_that_cannot_name_one_question_fall_back_to_the_property_key() {
    let message = form(
        &json!({
            "first":choice("First?", &[("a", "A", None)]),
            "second":choice("Second?", &[("b", "B", None)]),
            "third":choice("Third?", &[("c", "C", None)]),
        }),
        &[],
    );
    let asked = json!({"toolCallId":"call","rawInput":{"steps":[
        {"header":"Same","question":"First?"},
        {"header":"Same","question":"Second?"},
        {"header":"first","question":"Third?"}]}});

    let pending = projected(&message, Some(&asked));

    let headers = pending.request["input"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["header"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(headers, ["Same", "second", "third"]);
}

#[test]
fn answers_that_fit_no_question_are_rejected_and_others_pass_through() {
    let (message, asked) = questionnaire();
    let pending = projected(&message, Some(&asked));
    let rejected = |answers: Value| {
        answer(
            &pending,
            &json!({"behavior":"allow","updatedInput":{"answers":answers}}),
        )
    };

    assert_eq!(
        rejected(json!({"Unknown":"Rust"})),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        rejected(json!({"Storage":["Postgres"]})),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        rejected(json!({"Storage":7})),
        Err(AgentSessionError::Rejected)
    );
    for untouched in [
        json!({"behavior":"deny"}),
        json!({"behavior":"allow","updatedInput":{"content":{"storage":"pg"}}}),
    ] {
        assert_eq!(answer(&pending, &untouched), Ok(None));
    }
    let strict = projected(
        &form(
            &json!({"only":choice("Only?", &[("a", "A", None)])}),
            &["only"],
        ),
        None,
    );
    assert_eq!(
        answer(
            &strict,
            &json!({"behavior":"allow","updatedInput":{"answers":{"only":"Something else"}}}),
        ),
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn forms_with_typed_properties_keep_the_shared_projection_and_coercion() {
    let message = form(
        &json!({"ok":{"type":"boolean","title":"Proceed?"},"count":{"type":"integer"}}),
        &["ok", "count"],
    );
    let shared = acp_permissions::form(&message, "minimax-code").unwrap();

    let pending = projected(&message, None);

    assert_eq!(
        pending.request["input"]["questions"],
        shared.request["input"]["questions"]
    );
    let response = json!({"behavior":"allow","updatedInput":{"answers":{"ok":"true","count":"3"}}});
    assert_eq!(answer(&pending, &response), Ok(None));
    assert_eq!(
        acp_permissions::resolve(&pending, &response).unwrap()["result"]["content"],
        json!({"ok":true,"count":3})
    );
}

#[test]
fn required_choices_keep_their_free_text_as_a_separate_question() {
    let message = form(
        &json!({"language":choice("Language?", &[("rust", "Rust", None)]),
            "language__other":other("Language?")}),
        &["language"],
    );

    let pending = projected(&message, None);

    let questions = pending.request["input"]["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0]["allowOther"], false);
    assert_eq!(questions[0].get("otherKey"), None);
    let native = answer(
        &pending,
        &json!({"behavior":"allow","updatedInput":{"answers":{
            "language":"Rust","language__other":"Zig"}}}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        acp_permissions::resolve(&pending, &native).unwrap()["result"]["content"],
        json!({"language":"rust","language__other":"Zig"})
    );
}

#[test]
fn steps_match_untrimmed_questions_and_text_steps_hide_the_native_placeholder() {
    let message = form(
        &json!({
            "q1":{"type":"string","title":"What should it be called?","description":"Others..."},
            "q2":{"type":"string","title":"Why?","description":"One sentence is enough."},
        }),
        &[],
    );
    let asked = json!({"toolCallId":"call","rawInput":{"steps":[
        {"header":"Reason","question":"Why?\n","description":"One sentence is enough."},
        {"header":"Name","question":" What should it be called?"}]}});

    let pending = projected(&message, Some(&asked));

    let questions = pending.request["input"]["questions"].as_array().unwrap();
    assert_eq!(
        questions
            .iter()
            .map(|question| (&question["header"], &question["description"]))
            .collect::<Vec<_>>(),
        [
            (&json!("Reason"), &json!("One sentence is enough.")),
            (&json!("Name"), &Value::Null)
        ]
    );
    assert_eq!(pending.request["metadata"]["toolCallId"], "call");
}
