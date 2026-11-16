use serde_json::json;

use super::*;

#[test]
fn compatible_native_todo_tools_publish_confirmed_snapshots_without_overriding_native_plans() {
    let replay = |status: &str, native: bool| {
        let mut stream = Stream::default();
        stream.begin("turn".into());
        stream
            .update(
                &json!({"sessionUpdate":"tool_call","toolCallId":"todo-tool",
            "title":"todo_write","status":"pending","rawInput":{"todos":[
                {"content":"1. Build","status":"in_progress"}]}}),
            )
            .unwrap();
        if native {
            stream
                .update(&json!({"sessionUpdate":"plan","entries":[]}))
                .unwrap();
        }
        stream
            .update(
                &json!({"sessionUpdate":"tool_call_update","toolCallId":"todo-tool",
            "status":status}),
            )
            .unwrap();
        stream
            .events
            .into_iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some(entry),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let completed = replay("completed", false);
    assert_eq!(completed.len(), 2);
    assert_eq!(completed[0].item["type"], "tool_call");
    assert_eq!(completed[1].item["items"][0]["text"], "1. Build");
    assert_eq!(completed[1].key, replay("completed", false)[1].key);
    assert_eq!(replay("failed", false).len(), 1);
    let native = replay("completed", true);
    assert_eq!(native.len(), 2);
    assert_eq!(native[0].item, json!({"type":"todo","items":[]}));
    assert_eq!(native[1].item["type"], "tool_call");
}

#[test]
fn acp_plan_snapshots_flush_text_preserve_repeated_states_and_replay_stable_keys() {
    let updates = [
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"First"}}),
        json!({"sessionUpdate":"plan","entries":[{"content":"Build","status":"in_progress"}]}),
        json!({"sessionUpdate":"plan","entries":[]}),
        json!({"sessionUpdate":"plan","entries":[{"content":"Build","status":"in_progress"}]}),
    ];
    let replay = || {
        let mut stream = Stream::default();
        stream.begin("turn".into());
        for update in &updates {
            stream.update(update).unwrap();
        }
        stream
            .events
            .into_iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some(entry),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let first = replay();
    let second = replay();
    assert_eq!(
        first.iter().map(|entry| &entry.key).collect::<Vec<_>>(),
        second.iter().map(|entry| &entry.key).collect::<Vec<_>>()
    );
    assert_eq!(first.len(), 4);
    assert_eq!(first[0].item["text"], "First");
    assert_eq!(first[1].item, first[3].item);
    assert_ne!(first[1].key, first[3].key);
    assert_eq!(first[2].item, json!({"type":"todo","items":[]}));
    assert!(
        first
            .iter()
            .all(|entry| entry.turn_id.as_deref() == Some("turn"))
    );
}

#[test]
fn acp_compaction_updates_emit_start_and_failure_and_ignore_duplicate_states() {
    let mut stream = Stream::default();
    stream.begin("turn".to_owned());
    stream
        .update(&json!({"sessionUpdate":"compaction_update",
        "compactionId":"future","status":"future"}))
        .unwrap();
    assert!(stream.events.is_empty());
    for status in ["in_progress", "in_progress", "cancelled", "cancelled"] {
        stream
            .update(&json!({"sessionUpdate":"compaction_update",
            "compactionId":"compact","status":status}))
            .unwrap();
    }
    let statuses: Vec<_> = stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry.item["status"].as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(statuses, ["loading", "failed"]);
}

#[test]
fn image_output_is_materialized_and_large_tool_output_is_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let mut stream = Stream::new(crate::images::ImageStore::new(
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
