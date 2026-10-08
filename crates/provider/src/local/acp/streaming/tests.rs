use serde_json::json;

use super::*;

#[test]
fn image_output_is_materialized_and_large_tool_output_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let mut stream = Stream::new(crate::local::images::ImageStore::new(
        directory.path().join("images"),
    ));
    stream.begin("turn".to_owned());
    stream
        .update(&json!({"sessionUpdate":"agent_message_chunk","content":{
        "type":"image","data":"aGVsbG8=","mimeType":"image/png"}}))
        .unwrap();
    stream.flush();
    let image = match stream.events.pop_back().unwrap() {
        AgentTurnEvent::Timeline(entry) => entry,
        unexpected => panic!("unexpected event: {unexpected:?}"),
    };
    assert!(
        image.item["text"]
            .as_str()
            .unwrap()
            .starts_with("![Image](")
    );
    assert!(!image.item.to_string().contains("aGVsbG8="));
    assert_eq!(
        std::fs::read_dir(directory.path().join("images"))
            .unwrap()
            .count(),
        1
    );
    let content = tool_content(
        &stream.images,
        &json!([{"type":"content","content":{
        "type":"image","data":"aGVsbG8=","mimeType":"image/png"}}]),
    )
    .unwrap();
    assert!(
        content[0]["content"]["text"]
            .as_str()
            .unwrap()
            .starts_with("![Image](")
    );
    assert!(!content.to_string().contains("aGVsbG8="));
    stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"read","status":"completed","rawOutput":"large output ".repeat(10000)})).unwrap();
    let result = match stream.events.pop_back().unwrap() {
        AgentTurnEvent::Timeline(entry) => entry,
        unexpected => panic!("unexpected event: {unexpected:?}"),
    };
    assert!(
        result.item["detail"]["output"]
            .as_str()
            .unwrap()
            .contains("Output truncated")
    );
    assert!(result.item.to_string().len() < 256 * 1024);
    assert!(stream.tools.is_empty());
    assert!(stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"completed"})).is_err());
}

#[test]
fn groups_adjacent_chunks_and_keeps_tool_snapshots_complete() {
    let mut stream = Stream::default();
    stream.begin("turn-one".to_owned());
    for text in ["hello", " world"] {
        stream.update(&json!({"sessionUpdate":"agent_message_chunk","messageId":"message", "content":{"type":"text","text":text}})).unwrap();
    }
    stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"shell","status":"in_progress","rawInput":{"command":"pwd"}})).unwrap();
    stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"failed","rawOutput":"failure"})).unwrap();
    let entries: Vec<_> = stream
        .events
        .iter()
        .filter_map(|event| {
            if let AgentTurnEvent::Timeline(entry) = event {
                Some(entry)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].item["text"], "hello world");
    assert_eq!(entries[1].item["detail"]["input"], json!({"command":"pwd"}));
    assert_eq!(entries[1].item["detail"]["output"], "failure");
    assert_eq!(entries[1].item["status"], "failed");
    assert_eq!(stream.last_message.as_deref(), Some("hello world"));
}

#[test]
fn rejects_invalid_usage_and_oversized_text() {
    let mut stream = Stream::default();
    stream.begin("turn".to_owned());
    assert!(
        stream
            .update(&json!({"sessionUpdate":"usage_update","used":-1,"size":10}))
            .is_err()
    );
    assert!(
        stream
            .update(
                &json!({"sessionUpdate":"usage_update","used":9_007_199_254_740_992_u64,"size":10})
            )
            .is_err()
    );
    assert!(stream.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"image","data":"not text"}})).is_err());
    assert!(stream.update(&json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x".repeat(MAX_TEXT + 1)}})).is_err());
}

#[test]
fn prompt_usage_replaces_counters_and_retains_native_context() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream
        .update(&json!({"sessionUpdate":"usage_update","used":123,"size":1000}))
        .unwrap();
    for _ in 0..2 {
        stream
            .prompt_usage(
                &json!({"usage":{"inputTokens":42,"outputTokens":7,"cachedReadTokens":12}}),
            )
            .unwrap();
    }
    assert_eq!(stream.usage.input_tokens, Some(42));
    assert_eq!(stream.usage.cached_input_tokens, Some(12));
    assert_eq!(stream.usage.output_tokens, Some(7));
    assert_eq!(stream.usage.context_window_used_tokens, Some(123));
    for usage in [
        json!([]),
        json!({"inputTokens":-1}),
        json!({"outputTokens":9_007_199_254_740_992_u64}),
    ] {
        assert!(stream.prompt_usage(&json!({"usage":usage})).is_err());
    }
    stream.prompt_usage(&json!({"usage":null})).unwrap();
}

#[test]
fn interrupted_turn_finalizes_tool_snapshot_without_discarding_input() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"shell","status":"in_progress","rawInput":{"command":"sleep"}})).unwrap();
    assert!(stream.has_unfinished_tools());
    stream.finish("Turn cancelled").unwrap();
    assert!(!stream.has_unfinished_tools());
    let AgentTurnEvent::Timeline(entry) = stream.events.pop_back().unwrap() else {
        panic!("missing tool")
    };
    assert_eq!(entry.item["detail"]["input"]["command"], "sleep");
    assert_eq!(entry.item["status"], "failed");
    assert_eq!(entry.item["error"]["message"], "Turn cancelled");
}

#[test]
fn plan_updates_project_tasks_and_reject_invalid_statuses() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream.update(&json!({"sessionUpdate":"plan","entries":[{"content":"Implement","status":"in_progress"},{"content":"Verify","status":"completed"}]})).unwrap();
    let AgentTurnEvent::Timeline(entry) = stream.events.pop_back().unwrap() else {
        panic!("missing plan")
    };
    assert_eq!(
        entry.item["items"],
        json!([{"text":"Implement","completed":false},{"text":"Verify","completed":true}])
    );
    for plan in [
        json!({"entries":null}),
        json!({"entries":[{"content":"Task","status":"invalid"}]}),
        json!({"entries":[{"content":"x".repeat(MAX_TEXT + 1),"status":"pending"}]}),
    ] {
        let mut plan = plan;
        plan["sessionUpdate"] = json!("plan");
        assert!(stream.update(&plan).is_err());
    }
}
