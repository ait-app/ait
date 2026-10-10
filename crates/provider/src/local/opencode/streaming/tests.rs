use super::*;

#[test]
fn wrapped_native_output_is_unwrapped_before_utf8_bounding() {
    let output = "文".repeat(100_000);
    let mut stream = Stream::default();
    for update in [
        json!({"sessionUpdate":"tool_call","toolCallId":"shell","kind":"execute","title":"pwd","status":"pending","rawInput":{"command":"pwd"}}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"shell","status":"completed","rawOutput":{"metadata":{"exit":0},"output":output}}),
    ] {
        stream.update(&update).unwrap();
    }
    for event in stream.events {
        let (AgentTurnEvent::Progress { entry, .. } | AgentTurnEvent::Timeline(entry)) = event
        else {
            continue;
        };
        if entry.item["status"] == "running" {
            assert!(entry.item["detail"].get("output").is_none());
        } else {
            let text = entry.item["detail"]["output"].as_str().unwrap();
            assert!(text.starts_with("文文"));
            assert!(text.contains("Output truncated"));
            assert!(!text.contains("metadata"));
        }
        assert!(serde_json::to_vec(&entry).unwrap().len() < 768 * 1024);
    }
}

#[test]
fn native_content_text_is_bounded_without_turning_it_into_json_or_hiding_failures() {
    for (status, text) in [
        ("completed", "文".repeat(100_000)),
        ("failed", "Permission denied".into()),
    ] {
        let mut stream = Stream::default();
        stream.update(&json!({"sessionUpdate":"tool_call","toolCallId":"shell","kind":"execute","title":"pwd","status":"pending","rawInput":{"command":"pwd"}})).unwrap();
        stream.update(&json!({"sessionUpdate":"tool_call_update","toolCallId":"shell","status":status,
            "rawOutput":{"metadata":{}},"content":[{"type":"content","content":{"type":"text","text":text}}]})).unwrap();
        let entry = stream
            .events
            .into_iter()
            .find_map(|event| {
                if let AgentTurnEvent::Timeline(entry) = event {
                    Some(entry)
                } else {
                    None
                }
            })
            .unwrap();
        let output = entry.item["detail"]["output"].as_str().unwrap();
        assert!(text.starts_with(output.split('\n').next().unwrap()));
        assert!(serde_json::to_vec(&entry).unwrap().len() < 768 * 1024);
        if status == "failed" {
            assert_eq!(entry.item["error"], text);
        }
    }
}

#[test]
fn long_utf8_text_is_split_deterministically_without_losing_the_terminal_message() {
    let text = "文".repeat(100_000);
    let mut stream = Stream::default();
    stream.update(&json!({"sessionUpdate":"agent_message_chunk","messageId":"answer","content":{"type":"text","text":text}})).unwrap();
    stream.flush();
    assert_eq!(stream.last_message.as_deref(), Some(text.as_str()));
    let items = stream
        .events
        .into_iter()
        .filter_map(|event| {
            if let AgentTurnEvent::Timeline(entry) = event {
                Some(entry)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 4);
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.item["text"].as_str().unwrap())
            .collect::<String>(),
        text
    );
    for item in &items {
        assert!(serde_json::to_vec(item).unwrap().len() < 768 * 1024);
    }
    let mut chunked = Stream::default();
    for chunk in ["文".repeat(30_000), "文".repeat(70_000)] {
        chunked.update(&json!({"sessionUpdate":"agent_message_chunk","messageId":"answer","content":{"type":"text","text":chunk}})).unwrap();
    }
    chunked.flush();
    let replay = chunked
        .events
        .into_iter()
        .filter_map(|event| {
            if let AgentTurnEvent::Timeline(entry) = event {
                Some(entry)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        items
            .iter()
            .map(|entry| (&entry.key, &entry.item))
            .collect::<Vec<_>>(),
        replay
            .iter()
            .map(|entry| (&entry.key, &entry.item))
            .collect::<Vec<_>>()
    );
}

#[test]
fn image_uri_replay_preserves_native_links_without_reading_project_files() {
    let mut stream = Stream::default();
    stream.update(&json!({"sessionUpdate":"user_message_chunk","messageId":"user","content":{"type":"image","mimeType":"image/png","uri":"file:///work/image.png"}})).unwrap();
    stream.flush();
    let item = stream
        .events
        .into_iter()
        .find_map(|event| {
            if let AgentTurnEvent::Timeline(entry) = event {
                Some(entry)
            } else {
                None
            }
        })
        .unwrap();
    assert!(
        item.item["text"]
            .as_str()
            .unwrap()
            .contains("/work/image.png")
    );
}

#[test]
fn acp_tools_keep_native_order_between_commentary_and_final_text() {
    let mut stream = Stream::default();
    for update in [
        json!({"sessionUpdate":"agent_message_chunk","messageId":"before","content":{"type":"text","text":"Before"}}),
        json!({"sessionUpdate":"tool_call","toolCallId":"tool","title":"read","kind":"read","status":"pending","rawInput":{"path":"file"}}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"tool","status":"completed","rawOutput":"Output"}),
        json!({"sessionUpdate":"agent_message_chunk","messageId":"after","content":{"type":"text","text":"After"}}),
    ] {
        stream.update(&update).unwrap();
    }
    stream.flush();
    assert!(stream.events.iter().any(|event| matches!(event, AgentTurnEvent::Progress { entry, .. } if entry.item["type"] == "tool_call" && entry.item["status"] == "completed")));
    let items = stream
        .events
        .into_iter()
        .filter_map(|event| {
            if let AgentTurnEvent::Timeline(entry) = event {
                Some(entry)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        items
            .iter()
            .map(|entry| entry.item["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["assistant_message", "tool_call", "assistant_message"]
    );
    assert_eq!(stream.last_message.as_deref(), Some("After"));
}

#[test]
fn a_user_turn_without_assistant_text_does_not_reuse_a_previous_answer() {
    let mut stream = Stream::default();
    stream.update(&json!({"sessionUpdate":"agent_message_chunk","messageId":"old","content":{"type":"text","text":"Previous answer"}})).unwrap();
    stream.update(&json!({"sessionUpdate":"user_message_chunk","messageId":"new","content":{"type":"text","text":"Next input"}})).unwrap();
    stream.flush();
    assert!(stream.last_message.is_none());
}

#[test]
fn json_escaped_text_segments_fit_the_persisted_timeline_budget() {
    let mut stream = Stream::default();
    stream.update(&json!({"sessionUpdate":"agent_message_chunk","messageId":"answer","content":{"type":"text","text":"\0".repeat(160_000)}})).unwrap();
    stream.flush();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let mut count = 0;
    for event in stream.events {
        if let AgentTurnEvent::Timeline(entry) = event {
            timeline
                .append("escaped-text", "opencode", &[entry])
                .unwrap();
            count += 1;
        }
    }
    assert_eq!(count, 2);
}
