use super::*;

#[test]
fn compatible_native_task_tools_share_snapshots_without_provider_specific_rules() {
    for name in ["TodoWrite", "todo_write", "todowrite"] {
        let input = json!({"todos":[{"id":"native","content":"1.2 Keep numbering",
            "status":"in_progress","activeForm":"Working"}]});
        let projected = tool_input(name, &input).unwrap();
        assert_eq!(
            projected["items"][0],
            json!({"id":"native","text":"1.2 Keep numbering",
            "status":"in_progress","completed":false,"activeForm":"Working"})
        );
        assert_eq!(Some(projected), tool_input(name, &json!(input.to_string())));
        assert_eq!(
            tool_input(name, &json!({"todos":[]})),
            Some(json!({"type":"todo","items":[]}))
        );
    }
    assert_eq!(
        tool_input(
            "update_plan",
            &json!({"plan":[{"step":"Plan","status":"inProgress"}]})
        )
        .unwrap()["items"][0]["status"],
        "in_progress"
    );
    assert!(tool_input("other", &json!({"todos":[]})).is_none());
    assert!(
        tool_input(
            "TodoWrite",
            &json!({"todos":[{"content":"Bad","status":"future"}]})
        )
        .is_none()
    );
    assert!(tool_input("TodoWrite", &json!("{")).is_none());
}

#[test]
fn native_plans_preserve_each_status_and_empty_snapshots_clear_the_list() {
    let projected = snapshot(&json!([
        {"content":"Read","priority":"high","status":"completed"},
        {"content":"Implement","priority":"medium","status":"in_progress"},
        {"content":"Verify","priority":"low","status":"pending"}
    ]))
    .unwrap();
    assert_eq!(
        projected,
        json!({"type":"todo","items":[
            {"text":"Read","status":"completed","completed":true},
            {"text":"Implement","status":"in_progress","completed":false},
            {"text":"Verify","status":"pending","completed":false}
        ]})
    );
    assert_eq!(
        snapshot(&json!([])).unwrap(),
        json!({"type":"todo","items":[]})
    );
}

#[test]
fn malformed_or_oversized_plans_fail_before_becoming_display_snapshots() {
    for update in [
        json!({}),
        json!({"entries":false}),
        json!({"entries":[{"status":"pending"}]}),
        json!({"entries":[{"content":"Future","status":"future"}]}),
        json!({"entries":[{"content":"x".repeat(MAX_TEXT_BYTES + 1),"status":"pending"}]}),
        json!({"entries":vec![json!({"content":"x","status":"pending"});MAX_ITEMS + 1]}),
        json!({"entries":vec![json!({"content":"\n".repeat(MAX_TEXT_BYTES),"status":"pending"});16]}),
    ] {
        assert_eq!(snapshot(&update["entries"]), Err(AgentSessionError::Failed));
    }
}
