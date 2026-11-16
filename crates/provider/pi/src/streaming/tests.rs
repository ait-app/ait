use super::*;

#[test]
fn successful_native_todo_extension_results_emit_snapshots_but_failures_do_not() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    for (id, todos, failed) in [
        ("add", json!([{"id":1,"text":"Build","done":false}]), false),
        ("failed", json!([]), true),
        ("clear", json!([]), false),
    ] {
        stream
            .consume(&json!({"type":"tool_execution_start","toolCallId":id,
            "toolName":"todo","args":{"action":"list"}}))
            .unwrap();
        stream
            .consume(&json!({"type":"tool_execution_end","toolCallId":id,
            "toolName":"todo","isError":failed,"result":{"content":[],"details":{"todos":todos}}}))
            .unwrap();
    }
    let snapshots = stream
        .events()
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "todo" => Some(entry),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].item["items"][0]["id"], "1");
    assert_eq!(snapshots[0].key, "native:pi:turn:todo:add");
    assert_eq!(snapshots[1].item, json!({"type":"todo","items":[]}));
}

#[test]
fn compatible_native_todo_tool_extensions_emit_canonical_snapshots_only_after_success() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    for (id, failed) in [("ok", false), ("failed", true)] {
        stream
            .consume(
                &json!({"type":"tool_execution_start","toolCallId":id,"toolName":"TodoWrite",
            "args":{"todos":[{"content":"1. Build","status":"pending"}]}}),
            )
            .unwrap();
        stream
            .consume(
                &json!({"type":"tool_execution_end","toolCallId":id,"toolName":"TodoWrite",
            "isError":failed,"result":{"content":[]}}),
            )
            .unwrap();
    }
    let snapshots = stream
        .events()
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "todo" => Some(entry),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].item["items"][0]["text"], "1. Build");
}

#[test]
fn submitted_batch_preserves_cleanup_capacity_and_rejects_atomically() {
    for admission in [false, true] {
        let mut stream = Stream::default();
        stream.begin("turn".into());
        stream.admission = admission;
        let limit = stream.event_limit();
        for _ in 0..limit - 3 {
            stream.push(AgentTurnEvent::Cancelled).unwrap();
        }
        let batch = Stream::user_entries("turn", None, &"x".repeat(MAX_TEXT + 1), &[]);
        assert_eq!(
            stream.insert_submitted(batch),
            Err(AgentSessionError::Failed)
        );
        assert_eq!(stream.events.len(), limit - 3);
        assert_eq!(stream.input_position, 0);
        stream
            .insert_submitted(Stream::user_entries("turn", None, "input", &[]))
            .unwrap();
        assert_eq!(stream.events.len() + stream.tools.len() + 2, limit);
        assert!(matches!(
            stream.events.front(),
            Some(AgentTurnEvent::Timeline(_))
        ));
        assert_eq!(stream.input_position, 1);
    }
}

#[test]
fn submitted_input_reclaims_notice_capacity() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&json!({"type":"extension_ui_request", "method":"notify", "message":"notice"}))
        .unwrap();
    assert_eq!(stream.queued_notices.len(), 1);
    while stream.events.len() + 2 < stream.event_limit() {
        stream.push(AgentTurnEvent::Cancelled).unwrap();
    }
    stream
        .insert_submitted(Stream::user_entries("turn", None, "input", &[]))
        .unwrap();
    assert!(stream.queued_notices.is_empty());
    assert_eq!(stream.events.len() + 2, stream.event_limit());
    assert!(matches!(
        stream.events.front(),
        Some(AgentTurnEvent::Timeline(_))
    ));
}

fn assistant_end(usage: &Value) -> Value {
    json!({"type":"message_end","message":{"role":"assistant","model":"m",
        "content":[{"type":"text","text":"hi"}],"stopReason":"stop","usage":usage}})
}

fn requests(stream: &Stream) -> Vec<&provider_core::ports::usage_ledger::RequestUsage> {
    stream
        .events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::RequestUsage(request) => Some(request),
            _ => None,
        })
        .collect()
}

#[test]
fn assistant_messages_queue_one_request_each_before_the_terminal_event() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    for usage in [
        json!({"input":12,"output":3,"cacheRead":2,"cacheWrite":1,"cost":{"total":0.5}}),
        json!({"input":7,"output":1,"cost":{"total":"free"}}),
    ] {
        stream
            .consume(&json!({"type":"message_start","message":{"role":"assistant"}}))
            .unwrap();
        stream.consume(&assistant_end(&usage)).unwrap();
    }
    stream
        .consume(&json!({"type":"agent_settled","aborted":false}))
        .unwrap();

    let queued = requests(&stream);
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].segment_id, "pi:turn:1");
    assert_eq!(queued[1].segment_id, "pi:turn:2");
    assert_eq!(
        (
            queued[0].input_tokens,
            queued[0].cache_read_tokens,
            queued[0].cache_write_tokens
        ),
        (12, 2, 1)
    );
    assert_eq!(queued[0].cost_usd_reported, Some(0.5));
    assert_eq!(queued[1].cost_usd_reported, None);
    assert!(matches!(
        stream.events.back(),
        Some(AgentTurnEvent::Completed(_))
    ));
}

#[test]
fn aborted_zero_usage_queues_no_request_but_keeps_the_snapshot() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    stream
        .consume(&assistant_end(
            &json!({"input":0,"output":0,"cacheRead":0,"cost":{"total":0}}),
        ))
        .unwrap();

    assert!(requests(&stream).is_empty());
    assert!(
        stream
            .events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Usage(_)))
    );
}

#[test]
fn a_full_display_queue_drops_the_request_instead_of_failing_the_turn() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    while stream.events.len() + stream.tools.len() + 4 + REQUEST_HEADROOM < stream.event_limit() {
        stream.push(AgentTurnEvent::Cancelled).unwrap();
    }

    stream
        .consume(&assistant_end(&json!({"input":1,"output":1})))
        .unwrap();

    assert!(requests(&stream).is_empty());
    assert!(stream.failure.is_none());
}

#[test]
fn a_queued_request_leaves_headroom_so_later_output_still_fits() {
    let mut stream = Stream::default();
    stream.begin("turn".into());
    while stream.events.len() + stream.tools.len() + 5 + REQUEST_HEADROOM < stream.event_limit() {
        stream.push(AgentTurnEvent::Cancelled).unwrap();
    }

    stream
        .consume(&assistant_end(&json!({"input":1,"output":1})))
        .unwrap();

    assert_eq!(requests(&stream).len(), 1);
    for _ in 0..REQUEST_HEADROOM {
        stream.push(AgentTurnEvent::Cancelled).unwrap();
    }
}
