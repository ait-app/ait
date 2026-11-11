use serde_json::{Value, json};

use super::{ClaudeRequests, CodexRequests};
use crate::ports::usage_ledger::{RequestUsage, UsageBoundary};

fn start(id: &str, usage: &Value) -> Value {
    json!({"type":"stream_event","event":{"type":"message_start",
        "message":{"id":id,"model":"claude-test","usage":usage}}})
}

fn delta(usage: &Value) -> Value {
    json!({"type":"stream_event","event":{"type":"message_delta","usage":usage}})
}

fn child(mut record: Value, parent: &str) -> Value {
    record["parent_tool_use_id"] = json!(parent);
    record
}

#[test]
fn claude_maps_disjoint_buckets_and_merges_split_frames_by_maximum() {
    let mut requests = ClaudeRequests::default();
    let usage = json!({"input_tokens":10,"cache_read_input_tokens":30,
        "cache_creation_input_tokens":20,"cache_creation":{"ephemeral_1h_input_tokens":5},
        "output_tokens":1});

    assert_eq!(requests.observe(&start("msg_1", &usage), "turn"), None);
    assert_eq!(requests.observe(&start("msg_1", &usage), "turn"), None);
    assert_eq!(
        requests.observe(&delta(&json!({"output_tokens":7})), "turn"),
        None
    );
    assert_eq!(
        requests.observe(&delta(&json!({"output_tokens":4})), "turn"),
        None
    );
    let emitted = requests.finish("turn", None);

    assert_eq!(
        emitted,
        [RequestUsage {
            segment_id: "claude:turn:msg_1".to_owned(),
            parent: None,
            model: Some("claude-test".to_owned()),
            boundary: UsageBoundary::Request,
            input_tokens: 10,
            output_tokens: 7,
            cache_read_tokens: 30,
            cache_write_tokens: 20,
            cache_write_1h_tokens: Some(5),
            reasoning_tokens: None,
            cost_usd_reported: None,
            complete: true,
        }]
    );
}

#[test]
fn claude_emits_a_message_when_the_next_one_starts_and_drops_late_duplicates() {
    let mut requests = ClaudeRequests::default();
    let first = start("msg_1", &json!({"input_tokens":3,"output_tokens":1}));

    requests.observe(&first, "turn");
    let assistant = json!({"type":"assistant","message":{"id":"msg_1",
        "usage":{"input_tokens":3,"output_tokens":2}}});
    assert_eq!(requests.observe(&assistant, "turn"), None);
    let emitted = requests
        .observe(&start("msg_2", &json!({"input_tokens":4})), "turn")
        .unwrap();
    assert_eq!(emitted.segment_id, "claude:turn:msg_1");
    assert_eq!(emitted.output_tokens, 2);
    assert!(!emitted.complete, "message_delta never arrived");

    assert_eq!(requests.observe(&first, "turn"), None);
    let emitted = requests.finish("turn", None);
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].segment_id, "claude:turn:msg_2");
}

#[test]
fn claude_sub_agent_frames_become_segments_with_their_parent_stream() {
    let mut requests = ClaudeRequests::default();

    requests.observe(&start("msg_root", &json!({"input_tokens":1})), "turn");
    requests.observe(
        &child(start("msg_child", &json!({"input_tokens":2})), "toolu_1"),
        "turn",
    );
    requests.observe(
        &child(delta(&json!({"output_tokens":9})), "toolu_1"),
        "turn",
    );
    let sidechain = json!({"type":"assistant","isSidechain":true,
        "message":{"id":"msg_orphan","usage":{"input_tokens":50}}});
    assert_eq!(requests.observe(&sidechain, "turn"), None);
    let emitted = requests.finish("turn", None);

    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[0].parent, None);
    assert_eq!(emitted[0].output_tokens, 0);
    assert_eq!(emitted[1].segment_id, "claude:turn:msg_child");
    assert_eq!(emitted[1].parent.as_deref(), Some("toolu_1"));
    assert_eq!(emitted[1].output_tokens, 9);
    assert!(emitted[1].complete);
}

#[test]
fn claude_result_without_message_frames_emits_turn_residual_once() {
    let mut requests = ClaudeRequests::default();
    let result = json!({"type":"result","usage":{"input_tokens":40,"output_tokens":8,
        "cache_read_input_tokens":100}});

    let emitted = requests.finish("turn", Some(&result));
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0].segment_id, "claude:turn:result:0");
    assert_eq!(emitted[0].boundary, UsageBoundary::Turn);
    assert_eq!((emitted[0].input_tokens, emitted[0].output_tokens), (40, 8));
    assert_eq!(emitted[0].cache_read_tokens, 100);

    requests.observe(
        &child(
            start("msg_child", &json!({"input_tokens":50,"output_tokens":3})),
            "toolu",
        ),
        "turn",
    );
    let emitted = requests.finish("turn", Some(&result));
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[1].segment_id, "claude:turn:result:1");
    assert_eq!(
        (emitted[1].input_tokens, emitted[1].output_tokens),
        (0, 5),
        "attributed tokens are subtracted and never go negative"
    );

    requests.observe(&start("msg_root", &json!({"input_tokens":1})), "turn");
    let emitted = requests.finish("turn", Some(&result));
    assert_eq!(
        emitted.len(),
        1,
        "per-message frames suppress the turn segment"
    );
    assert_eq!(emitted[0].boundary, UsageBoundary::Request);
}

#[test]
fn claude_drops_frames_without_identity_usage_or_open_message() {
    let mut requests = ClaudeRequests::default();

    requests.observe(&start("", &json!({"input_tokens":1})), "turn");
    requests.observe(
        &json!({"type":"stream_event","event":{"type":"message_start","message":{"id":"msg"}}}),
        "turn",
    );
    requests.observe(&delta(&json!({"output_tokens":1})), "turn");
    assert!(requests.finish("turn", None).is_empty());

    requests.observe(
        &start("msg", &json!({"input_tokens":1})),
        "x".repeat(512).as_str(),
    );
    assert!(requests.finish(&"x".repeat(512), None).is_empty());

    requests.begin();
    requests.observe(&start("msg", &json!({"input_tokens":1})), "next");
    assert_eq!(requests.finish("next", None).len(), 1);
}

#[test]
fn codex_maps_last_request_and_accepts_only_growing_totals() {
    let mut requests = CodexRequests::default();
    let usage = |total: u64| {
        json!({"total":{"totalTokens":total},"last":{"inputTokens":100,"cachedInputTokens":30,
            "outputTokens":12,"reasoningOutputTokens":5,"totalTokens":112}})
    };

    assert_eq!(
        requests.observe(&usage(500), "turn-1"),
        Some(RequestUsage {
            segment_id: "codex:turn-1:500".to_owned(),
            parent: None,
            model: None,
            boundary: UsageBoundary::Request,
            input_tokens: 70,
            output_tokens: 12,
            cache_read_tokens: 30,
            cache_write_tokens: 0,
            cache_write_1h_tokens: None,
            reasoning_tokens: Some(5),
            cost_usd_reported: None,
            complete: true,
        })
    );
    assert_eq!(requests.observe(&usage(500), "turn-1"), None);
    assert_eq!(requests.observe(&usage(400), "turn-1"), None);
    assert_eq!(
        requests.observe(&usage(612), "turn-2").unwrap().segment_id,
        "codex:turn-2:612"
    );
}

#[test]
fn codex_keeps_input_non_negative_and_omits_absent_reasoning() {
    let mut requests = CodexRequests::default();

    let request = requests
        .observe(
            &json!({"total":{"total_tokens":9},"last":{"input_tokens":3,"cached_input_tokens":8,
                "output_tokens":1}}),
            "turn",
        )
        .unwrap();
    assert_eq!(request.input_tokens, 0);
    assert_eq!(request.cache_read_tokens, 8);
    assert_eq!(request.reasoning_tokens, None);

    assert_eq!(
        requests.observe(&json!({"last":{"inputTokens":1}}), "turn"),
        None
    );
    assert_eq!(
        requests.observe(&json!({"total":{"totalTokens":20}}), "turn"),
        None
    );
    let partial = requests
        .observe(
            &json!({"total":{"totalTokens":21},"last":{"inputTokens":1}}),
            "turn",
        )
        .unwrap();
    assert!(!partial.complete);
    assert_eq!(
        requests.observe(
            &json!({"total":{"totalTokens":30},"last":{"inputTokens":1}}),
            &"t".repeat(512)
        ),
        None
    );
}
