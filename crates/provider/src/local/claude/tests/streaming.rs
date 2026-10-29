use super::*;
use crate::local::claude::streaming;
use crate::ports::agent_session::AgentTurnEvent;

#[test]
fn compaction_status_emits_one_loading_marker_for_each_boundary() {
    let mut stream = streaming::Stream::default();
    stream
        .record(&json!({"type":"system","subtype":"status","status":null}))
        .unwrap();
    stream
        .record(&json!({
            "type":"system", "subtype":"status", "status":"compacting",
            "uuid":"child", "parent_tool_use_id":"child-tool"
        }))
        .unwrap();
    assert!(stream.events.is_empty());

    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    for (index, with_uuid) in [(0, true), (1, false)] {
        let mut status = json!({"type":"system","subtype":"status","status":"compacting"});
        if with_uuid {
            status["uuid"] = json!("status");
        }
        stream.record(&status).unwrap();
        stream.record(&status).unwrap();
        stream
            .record(&json!({
                "type":"system", "subtype":"status", "status":"compacting",
                "uuid":"duplicate"
            }))
            .unwrap();
        assert_eq!(stream.events.len(), 1);
        stream
            .record(&json!({
                "type":"system", "subtype":"compact_boundary",
                "uuid":format!("boundary-{index}")
            }))
            .unwrap();
        assert_eq!(stream.events.len(), 2);
        for expected in ["loading", "completed"] {
            let AgentTurnEvent::Timeline(entry) = stream.events.pop_front().unwrap() else {
                panic!("compaction must emit durable timeline markers");
            };
            assert_eq!(entry.item["type"], "compaction");
            assert_eq!(entry.item["status"], expected);
            timeline.append("agent", "claude", &[entry]).unwrap();
        }
    }
    assert_eq!(timeline.read("agent").unwrap().1.len(), 4);
}

#[test]
fn structured_outputs_and_result_only_commands_are_visible_without_duplicating_streamed_text() {
    let mut stream = streaming::Stream::default();
    let structured = json!({"uuid":"result","structured_output":{"answer":42}});
    assert!(
        stream
            .result_text(&structured, "turn")
            .unwrap()
            .unwrap()
            .contains("42")
    );
    assert!(matches!(
        stream.events.pop_front(),
        Some(AgentTurnEvent::Timeline(_))
    ));
    stream.result_text(&structured, "turn").unwrap();
    assert!(stream.events.is_empty());
    assert_eq!(
        stream
            .result_text(&json!({"uuid":"command","result":"Command output"}), "turn")
            .unwrap()
            .as_deref(),
        Some("Command output")
    );
    assert_eq!(stream.events.len(), 1);
    stream.events.clear();
    stream.last_message = Some("streamed".into());
    assert_eq!(
        stream
            .result_text(&json!({"uuid":"final","result":"streamed"}), "turn")
            .unwrap()
            .as_deref(),
        Some("streamed")
    );
    assert!(stream.events.is_empty());
}

#[test]
fn tool_images_render_once_and_replay_with_the_same_identity() {
    let root = tempfile::tempdir().unwrap();
    let images = crate::local::images::ImageStore::new(root.path().join("images"));
    let records = [
        json!({"type":"assistant","uuid":"tool-msg","message":{"id":"api","content":[
            {"type":"tool_use","id":"call","name":"mcp__images__render","input":{}}]}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call","content":[
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"aGVsbG8="}}]}]}}),
    ];
    let replay = || {
        let mut stream = streaming::Stream::new(images.clone());
        for record in &records {
            stream.record(record).unwrap();
        }
        stream.record(&records[1]).unwrap();
        stream
            .events
            .into_iter()
            .filter_map(|event| match event {
                AgentTurnEvent::Timeline(entry) => Some((entry.key, entry.item)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let entries = replay();
    assert_eq!(entries, replay());
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].0, "native:claude:tool:call:image:0");
    assert_eq!(entries[1].1["type"], "assistant_message");
    assert!(!entries[0].1.to_string().contains("aGVsbG8="));
}

fn partial(stream: &mut streaming::Stream, event: &Value) {
    stream
        .record(&json!({"type":"stream_event","event":event}))
        .unwrap();
}

#[test]
fn text_and_reasoning_deltas_match_native_history_keys() {
    let mut stream = streaming::Stream::default();
    partial(
        &mut stream,
        &json!({"type":"message_start","message":{"id":"message"}}),
    );
    for (index, kind, field, text) in [
        (0, "thinking", "thinking", "Reason"),
        (1, "text", "text", "Hello"),
    ] {
        partial(
            &mut stream,
            &json!({"type":"content_block_start","index":index,"content_block":{"type":kind}}),
        );
        partial(
            &mut stream,
            &json!({"type":"content_block_delta","index":index,"delta":{"type":format!("{field}_delta"),(field):text}}),
        );
    }
    let record = json!({"type":"assistant","uuid":"record","message":{"id":"message","content":[
        {"type":"thinking","thinking":"Reason"},{"type":"text","text":"Hello"}]}});
    stream.record(&record).unwrap();
    stream.record(&record).unwrap();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    for event in stream.events {
        match event {
            AgentTurnEvent::Progress { observation, entry } => timeline
                .progress("agent", "claude", &observation, &entry)
                .unwrap(),
            AgentTurnEvent::Timeline(entry) => {
                timeline.append("agent", "claude", &[entry]).unwrap();
            }
            _ => panic!("unexpected stream event"),
        }
    }
    let (_, rows) = timeline.read("agent").unwrap();
    let content: String = rows
        .iter()
        .filter_map(|row| row.entry.item["text"].as_str())
        .collect();
    assert_eq!(content, "ReasonHello");
    assert_eq!(rows.len(), 4);
}

#[test]
fn tools_finish_from_user_results_and_children_stay_out_of_parent_text() {
    let mut stream = streaming::Stream::default();
    stream
        .record(
            &json!({"type":"assistant","uuid":"tool-msg","message":{"id":"api","content":[
        {"type":"tool_use","id":"call","name":"Bash","input":{"command":"pwd"}}]}}),
        )
        .unwrap();
    stream
        .record(&json!({"type":"assistant","parent_tool_use_id":"call","message":{}}))
        .unwrap();
    stream.record(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call","content":"denied","is_error":true}]}})).unwrap();
    assert!(stream.tools.is_empty());
    assert_eq!(stream.events.len(), 2);
    let AgentTurnEvent::Timeline(item) = stream.events.pop_back().unwrap() else {
        panic!()
    };
    assert_eq!(item.item["status"], "failed");
    assert_eq!(item.key, "native:claude:tool:call");
}

#[test]
fn injected_user_records_stay_hidden_but_still_finish_tools() {
    let mut stream = streaming::Stream::default();
    let handback = "Another Claude session sent a message:\n<agent-message from=\"a1\">\n[Subagent hand-back] report\n</agent-message>";
    for flag in ["isMeta", "isSynthetic"] {
        stream
            .record(&json!({"type":"user","uuid":flag,flag:true,"message":{"content":handback}}))
            .unwrap();
    }
    assert!(stream.events.is_empty());
    stream
        .record(
            &json!({"type":"assistant","uuid":"tool-msg","message":{"id":"api","content":[
        {"type":"tool_use","id":"call","name":"Bash","input":{"command":"pwd"}}]}}),
        )
        .unwrap();
    stream
        .record(
            &json!({"type":"user","uuid":"result","isSynthetic":true,"message":{"content":[
        {"type":"tool_result","tool_use_id":"call","content":"/tmp"},
        {"type":"text","text":"injected"}]}}),
        )
        .unwrap();
    assert!(stream.tools.is_empty());
    let AgentTurnEvent::Timeline(item) = stream.events.pop_back().unwrap() else {
        panic!()
    };
    assert_eq!(item.item["status"], "completed");
    assert!(stream.events.iter().all(|event| !matches!(event,
        AgentTurnEvent::Timeline(entry) if entry.item["type"] == "user_message")));
}

#[test]
fn subagent_handback_reports_show_as_the_final_message() {
    let mut stream = streaming::Stream::default();
    stream
        .record(
            &json!({"type":"assistant","uuid":"handback","message":{"id":"api","content":[
        {"type":"tool_use","id":"back","name":"SubagentHandback","input":{"message":"## Report\n- done"}},
        {"type":"tool_use","id":"empty","name":"SubagentHandback","input":{"message":"  "}}]}}),
        )
        .unwrap();
    stream
        .record(&json!({"type":"user","uuid":"ack","message":{"content":[
        {"type":"tool_result","tool_use_id":"back","content":"Delivered"},
        {"type":"tool_result","tool_use_id":"empty","content":"Delivered"}]}}))
        .unwrap();
    assert!(stream.tools.is_empty());
    let items: Vec<_> = stream
        .events
        .drain(..)
        .map(|event| match event {
            AgentTurnEvent::Timeline(entry) => entry.item,
            _ => panic!("handbacks must not stream tool rows"),
        })
        .collect();
    assert_eq!(
        items,
        [json!({"type":"assistant_message","messageId":"api:0","text":"## Report\n- done"})]
    );
    stream
        .result_text(&json!({"uuid":"r","result":"Delivered"}), "turn")
        .unwrap();
    assert!(stream.events.is_empty());
}

#[test]
fn malformed_and_excessive_output_fail_without_panicking() {
    let mut stream = streaming::Stream::default();
    assert!(
        stream
            .record(&json!({"type":"assistant","uuid":"bad","message":{}}))
            .is_err()
    );
    assert!(
        stream
            .record(
                &json!({"type":"stream_event","event":{"type":"content_block_delta","index":0}})
            )
            .is_err()
    );
    assert!(stream.record(&json!({"type":"assistant","uuid":"large","message":{"id":"large","content":[{"type":"text","text":"x".repeat(200*1024)}]}})).is_err());
    stream
        .record(&json!({"type":"system","subtype":"compact_boundary","uuid":"compact"}))
        .unwrap();
    assert!(matches!(
        stream.events.pop_back(),
        Some(AgentTurnEvent::Timeline(_))
    ));
}
