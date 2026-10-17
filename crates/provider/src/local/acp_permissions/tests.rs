use serde_json::{Value, json};

use super::{capture, resolve_tool};

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
    message["params"]["toolCall"]["rawInput"] = json!({"command":"x".repeat(65536)});
    assert!(capture(&message, "opencode").is_err());
}
