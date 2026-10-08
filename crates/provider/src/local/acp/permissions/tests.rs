use serde_json::json;

use super::*;

fn request() -> Value {
    json!({"id":42,"params":{"toolCall":{"toolCallId":"tool","title":"shell","rawInput":{"command":"pwd"}},
        "options":[{"optionId":"once","name":"Allow once","kind":"allow_once"},
            {"optionId":"always","name":"Allow always","kind":"allow_always"},
            {"optionId":"deny","name":"Deny","kind":"reject_once"}]}})
}

#[test]
fn maps_actual_permission_options_without_implicit_permanent_grants() {
    let pending = capture("deepseek-harness", &request()).unwrap();
    assert_eq!(
        resolve(
            &pending,
            &json!({"behavior":"deny","selectedActionId":"deny","message":"Denied by user"})
        )
        .unwrap()["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(
        resolve(&pending, &json!({"behavior":"allow"})).unwrap(),
        json!({"jsonrpc":"2.0","id":42,
        "result":{"outcome":{"outcome":"selected","optionId":"once"}}})
    );
    assert_eq!(
        resolve(&pending, &json!({"behavior":"deny"})).unwrap()["result"]["outcome"]["optionId"],
        "deny"
    );
    assert_eq!(
        resolve(
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
        assert!(resolve(&pending, &response).is_err());
    }
    let mut only_always = request();
    only_always["params"]["options"] =
        json!([{ "optionId":"always","name":"Allow always","kind":"allow_always" }]);
    assert!(
        resolve(
            &capture("deepseek-harness", &only_always).unwrap(),
            &json!({"behavior":"allow"})
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_or_unknown_permission_choices() {
    let mut duplicate = request();
    duplicate["params"]["options"][1]["optionId"] = json!("once");
    assert!(capture("deepseek-harness", &duplicate).is_err());
    let mut unknown = request();
    unknown["params"]["options"][0]["kind"] = json!("execute");
    assert!(capture("deepseek-harness", &unknown).is_err());
    assert!(capture("deepseek-harness", &json!({})).is_err());
}
