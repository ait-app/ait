use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{admit, cancelled, capture, form, resolve, resolve_tool};
use provider_core::ports::agent_session::AgentSessionError;

const PROVIDER: &str = "deepseek-harness";

fn request() -> Value {
    json!({"id":42,"params":{"toolCall":{"toolCallId":"tool","title":"shell","rawInput":{"command":"pwd"}},
        "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},
            {"optionId":"always","name":"Allow always","kind":"allow_always"},
            {"optionId":"deny","name":"Deny","kind":"reject_once"}]}})
}

#[test]
fn maps_actual_permission_options_without_implicit_permanent_grants() {
    let pending = capture(&request(), PROVIDER).unwrap();
    assert_eq!(
        resolve_tool(
            &pending,
            &json!({"behavior":"deny","selectedActionId":"deny","message":"Denied by user"})
        )
        .unwrap()["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(
        resolve_tool(&pending, &json!({"behavior":"allow"})).unwrap(),
        json!({"jsonrpc":"2.0","id":42,
        "result":{"outcome":{"outcome":"selected","optionId":"once"}}})
    );
    assert_eq!(
        resolve_tool(&pending, &json!({"behavior":"deny"})).unwrap()["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(
        resolve_tool(
            &pending,
            &json!({"behavior":"allow","selectedActionId":"always"})
        )
        .unwrap()["result"]["outcome"]["optionId"],
        "always"
    );
    for response in [
        json!({"behavior":"allow","selectedActionId":"deny"}),
        json!({"behavior":"allow","selectedActionId":"missing"}),
        json!({"behavior":"unknown"}),
        json!({"behavior":"allow","updatedInput":{}}),
    ] {
        assert!(resolve_tool(&pending, &response).is_err());
    }
    let mut only_always = request();
    only_always["params"]["options"] =
        json!([{ "optionId":"always","name":"Allow always","kind":"allow_always" }]);
    assert!(
        resolve_tool(
            &capture(&only_always, PROVIDER).unwrap(),
            &json!({"behavior":"allow"})
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_or_unknown_permission_choices() {
    let mut duplicate = request();
    duplicate["params"]["options"][1]["optionId"] = json!("once");
    assert!(capture(&duplicate, PROVIDER).is_err());
    let mut unknown = request();
    unknown["params"]["options"][0]["kind"] = json!("execute");
    assert!(capture(&unknown, PROVIDER).is_err());
    assert!(capture(&json!({}), PROVIDER).is_err());
}

#[test]
fn records_the_owning_provider_and_retains_no_elicitation_schema() {
    let pending = capture(&request(), PROVIDER).unwrap();
    assert_eq!(pending.request["provider"], json!(PROVIDER));
    assert_eq!(pending.request["kind"], json!("tool"));
    assert!(pending.schema.is_none());
    assert_eq!(pending.unique_fields, [] as [String; 0]);
}

#[test]
fn display_titles_preserve_multiline_scripts_without_relaxing_identity_validation() {
    for title in ["printf 'hello\\n'\npwd".to_owned(), "# 注释\n".repeat(300)] {
        let mut message = request();
        message["params"]["toolCall"]["title"] = json!(title);
        let pending = capture(&message, "opencode").unwrap();
        assert_eq!(pending.request["title"], title);
        assert_eq!(pending.request["name"], title);
        message["params"]["toolCall"]["toolCallId"] = json!("invalid\nidentity");
        assert!(capture(&message, "opencode").is_err());
    }
    for title in [json!(""), json!(" \n\t"), json!("bad\0title"), Value::Null] {
        let mut message = request();
        message["params"]["toolCall"]["title"] = title;
        assert!(capture(&message, "opencode").is_err());
    }
}

#[test]
fn native_full_file_previews_do_not_exhaust_the_retained_approval_budget() {
    let mut message = request();
    message["params"]["toolCall"]["content"] = json!([{
        "type":"diff", "path":"large.txt", "oldText":"a".repeat(100_000),
        "newText":"b".repeat(100_000)
    }]);
    let pending = capture(&message, "opencode").unwrap();
    assert_eq!(pending.request["input"], json!({"command":"pwd"}));
    assert!(pending.request.to_string().len() < 65536);
    assert_eq!(
        resolve_tool(&pending, &json!({"behavior":"deny"})).unwrap()["result"]["outcome"]["optionId"],
        "deny"
    );

    message["params"]["toolCall"]["rawInput"] = json!({"command":"x".repeat(50_000)});
    let pending = capture(&message, "opencode").unwrap();
    assert_eq!(
        pending.request["input"],
        message["params"]["toolCall"]["rawInput"]
    );
    assert!(pending.request.to_string().len() < 192 * 1024);
}

#[test]
fn oversized_titles_and_raw_input_become_bounded_previews_instead_of_failing() {
    let mut message = request();
    message["params"]["toolCall"]["title"] = json!("t".repeat(1024 * 1024));
    message["params"]["toolCall"]["rawInput"] =
        json!({"path":"large.txt","content":"x".repeat(1024 * 1024)});
    let pending = capture(&message, "opencode").unwrap();
    assert!(pending.request.to_string().len() < 192 * 1024);
    assert_eq!(pending.request["input"]["path"], "large.txt");
    for text in [
        &pending.request["title"],
        &pending.request["input"]["content"],
    ] {
        assert!(text.as_str().unwrap().contains("Output truncated"));
    }
    assert_eq!(pending.request["detail"]["input"], pending.request["input"]);
    assert_eq!(
        resolve_tool(&pending, &json!({"behavior":"allow"})).unwrap()["result"]["outcome"]["optionId"],
        "once"
    );
}

#[test]
fn cancelled_replies_withdraw_permissions_and_forms_without_a_choice() {
    let mut pending = capture(&request(), PROVIDER).unwrap();
    assert_eq!(
        cancelled(&pending),
        json!({"jsonrpc":"2.0","id":42,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    pending.schema = Some(json!({"type":"object"}));
    assert_eq!(
        cancelled(&pending),
        json!({"jsonrpc":"2.0","id":42,"result":{"action":"cancel"}})
    );
}

fn questionnaire() -> Value {
    json!({"id":"question","method":"elicitation/create","params":{
        "sessionId":"native","toolCallId":"tool-question","mode":"form",
        "message":"Choose languages","requestedSchema":{"type":"object",
            "properties":{
                "language":{"type":"array","uniqueItems":true,"items":{"anyOf":[
                    {"const":"rust","title":"Rust, stable"},{"const":"go","title":"Go"}]}},
                "language_custom":{"type":"string","title":"Other language"}},
            "required":["language"]}}})
}

#[test]
fn forms_project_native_choices_into_questions_for_the_owning_provider() {
    let pending = form(&questionnaire(), "minimax").unwrap();
    assert_eq!(pending.rpc_id, json!("question"));
    assert!(pending.options.is_empty());
    assert_eq!(pending.unique_fields, ["language"]);
    let schema = pending.schema.as_ref().unwrap();
    assert_eq!(
        schema["properties"]["language"],
        json!({"type":"array","items":{"type":"string","enum":["rust","go"],
            "enumNames":["Rust, stable","Go"]}})
    );
    let request = &pending.request;
    assert_eq!(request["provider"], "minimax");
    assert_eq!(request["kind"], "question");
    assert_eq!(request["title"], "Choose languages");
    assert_eq!(request["metadata"]["toolCallId"], "tool-question");
    let questions = request["input"]["questions"].as_array().unwrap();
    let language = questions
        .iter()
        .find(|question| question["header"] == "language")
        .unwrap();
    assert_eq!(language["allowEmpty"], false);
    assert_eq!(language["answerFormat"], "array");
    assert_eq!(language["answerKey"], "language");
    assert_eq!(language["allowOther"], false);
    assert_eq!(
        language["options"],
        json!([{"label":"Rust, stable"},{"label":"Go"}])
    );
    let custom = questions
        .iter()
        .find(|question| question["header"] == "language_custom")
        .unwrap();
    assert_eq!(custom["allowEmpty"], true);
}

#[test]
fn forms_reject_other_modes_unsupported_schemas_and_missing_identities() {
    let mut url = questionnaire();
    url["params"]["mode"] = json!("url");
    assert_eq!(
        form(&url, "minimax").unwrap_err(),
        AgentSessionError::Rejected
    );
    let mut nested = questionnaire();
    nested["params"]["requestedSchema"]["properties"]["language"] = json!({"type":"object"});
    assert_eq!(
        form(&nested, "minimax").unwrap_err(),
        AgentSessionError::Rejected
    );
    let mut unnamed = questionnaire();
    unnamed["params"]["requestedSchema"]["properties"]["language"]["items"]["anyOf"][0] =
        json!({"title":"Rust"});
    assert_eq!(
        form(&unnamed, "minimax").unwrap_err(),
        AgentSessionError::Rejected
    );
    let mut anonymous = questionnaire();
    anonymous["id"] = Value::Null;
    assert_eq!(
        form(&anonymous, "minimax").unwrap_err(),
        AgentSessionError::Failed
    );
}

#[test]
fn form_answers_map_titles_to_native_values_and_keep_unique_choices() {
    let pending = form(&questionnaire(), "opencode").unwrap();
    assert_eq!(
        resolve(
            &pending,
            &json!({"behavior":"allow","updatedInput":{"answers":{
                "language":["Rust, stable","Go"],"language_custom":"C++"}}})
        )
        .unwrap(),
        json!({"jsonrpc":"2.0","id":"question","result":{"action":"accept",
            "content":{"language":["rust","go"],"language_custom":"C++"}}})
    );
    assert_eq!(
        resolve(
            &pending,
            &json!({"behavior":"allow","updatedInput":{"content":{"language":["go"]}}})
        )
        .unwrap()["result"]["content"],
        json!({"language":["go"]})
    );
    assert_eq!(
        resolve(&pending, &json!({"behavior":"deny"})).unwrap()["result"],
        json!({"action":"decline"})
    );
    for response in [
        json!({"behavior":"allow","updatedInput":{"answers":{"language":["invalid"]}}}),
        json!({"behavior":"allow","updatedInput":{"content":{"language":["rust","rust"]}}}),
        json!({"behavior":"allow","updatedInput":{}}),
        json!({"behavior":"unknown"}),
    ] {
        assert_eq!(
            resolve(&pending, &response).unwrap_err(),
            AgentSessionError::Rejected
        );
    }
}

#[test]
fn resolve_answers_tool_permissions_with_their_native_options() {
    let pending = capture(&request(), PROVIDER).unwrap();
    assert_eq!(
        resolve(&pending, &json!({"behavior":"allow"})).unwrap(),
        resolve_tool(&pending, &json!({"behavior":"allow"})).unwrap()
    );
    assert!(resolve(&pending, &json!({"behavior":"allow","updatedInput":{}})).is_err());
}

#[test]
fn admit_retains_callbacks_only_for_the_active_prompt_of_the_session() {
    let mut pending = BTreeMap::new();
    let project = |message: &Value| form(message, "minimax");
    assert_eq!(
        admit(&questionnaire(), "native", false, &mut pending, project),
        Err(AgentSessionError::Failed)
    );
    assert_eq!(
        admit(&questionnaire(), "other", true, &mut pending, project),
        Err(AgentSessionError::Failed)
    );
    let admitted = admit(&questionnaire(), "native", true, &mut pending, project).unwrap();
    assert_eq!(admitted["kind"], "question");
    assert_eq!(pending[admitted["id"].as_str().unwrap()].request, admitted);
    assert_eq!(
        admit(&questionnaire(), "native", true, &mut pending, project),
        Err(AgentSessionError::Failed)
    );
    let mut permission = request();
    permission["params"]["sessionId"] = json!("native");
    admit(&permission, "native", true, &mut pending, |message| {
        capture(message, PROVIDER)
    })
    .unwrap();
    assert_eq!(pending.len(), 2);
    let mut rejected = questionnaire();
    rejected["id"] = json!("another");
    rejected["params"]["mode"] = json!("url");
    assert_eq!(
        admit(&rejected, "native", true, &mut pending, project),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(pending.len(), 2);
}
