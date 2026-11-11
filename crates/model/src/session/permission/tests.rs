use serde_json::{Value, json};

use super::project;

fn secret_request() -> Value {
    json!({
        "id": "dialog", "provider": "omp", "name": "extension_ui", "kind": "question",
        "title": "sensitive-title", "description": "sensitive-description",
        "input": {"questions": [
            {"question": "Public", "secret": false},
            {"question": "Private", "secret": true, "initialValue": "sensitive-prefill"}
        ], "answers": {"value": "sensitive-answer"}},
        "detail": {"input": "sensitive-detail"}, "metadata": {"value": "sensitive-metadata"},
        "suggestions": [{"value": "sensitive-suggestion"}],
        "actions": [{"id": "allow", "label": "Submit", "behavior": "allow"}],
        "futureField": "sensitive-future"
    })
}

#[test]
fn legacy_secret_request_retains_only_cancel_routing_and_fixed_explanation() {
    let mut request = secret_request();
    project(&mut request, false);
    assert_eq!(request["id"], "dialog");
    assert_eq!(request["provider"], "omp");
    assert_eq!(request["kind"], "tool");
    assert!(request["description"].as_str().unwrap().contains("Upgrade"));
    assert_eq!(
        request["actions"],
        json!([{"id": "deny", "label": "Cancel", "behavior": "deny"}])
    );
    for field in ["input", "detail", "metadata", "suggestions", "futureField"] {
        assert!(request.get(field).is_none(), "unexpected field: {field}");
    }
    assert!(!request.to_string().contains("sensitive-"));
    let once = request.clone();
    project(&mut request, false);
    assert_eq!(request, once);
}

#[test]
fn capable_secret_and_legacy_public_requests_are_unchanged() {
    let original = secret_request();
    let mut capable = original.clone();
    project(&mut capable, true);
    assert_eq!(capable, original);
    let mut public = original;
    public["input"]["questions"][1]["secret"] = json!(false);
    let original_public = public.clone();
    project(&mut public, false);
    assert_eq!(public, original_public);
}

#[test]
fn unrelated_permissions_and_absent_questions_are_unchanged() {
    for original in [
        Value::Null,
        json!({}),
        json!({"id": "tool", "kind": "tool", "input": {"command": "echo hello"}}),
        json!({"id": "question", "kind": "question", "input": {"questions": []}}),
        json!({"id": "question", "kind": "question", "input": {"questions": [{"question": "Public"}]}}),
    ] {
        let mut projected = original.clone();
        project(&mut projected, false);
        assert_eq!(projected, original);
    }
}
