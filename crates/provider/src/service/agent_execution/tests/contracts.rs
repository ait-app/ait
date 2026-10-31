//! Public execution contracts with a deterministic native adapter and isolated durable storage.

use serde_json::json;

use crate::protocol::prompt::AgentPrompt;
use crate::test_support::request;

mod fixture;
use fixture::Harness;

#[tokio::test]
async fn unsupported_rewind_preserves_the_turn_writer_and_queued_input() {
    let harness = Harness::new().await;
    harness.send().await;
    let timeline = harness.execution.timeline();
    timeline
        .reserve_input(
            &harness.id,
            "queued",
            &AgentPrompt::text("next"),
            "interrupt",
        )
        .unwrap();
    for mode in ["conversation", "files", "both"] {
        let result = request(
            &harness.execution,
            "agent.rewind.request",
            json!({
                "agentId":harness.id, "messageId":"previous", "mode":mode
            }),
        )
        .await;
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"], "UnsupportedCapability");
    }
    let snapshot = harness.snapshot().await;
    let queued = timeline.has_queued_input(&harness.id).unwrap();
    let (cancelled, closed) = {
        let state = harness.state.lock().unwrap();
        (state.cancellations, state.closes)
    };
    harness.execution.shutdown().await.unwrap();
    assert_eq!(cancelled, 0, "unsupported rewind cancelled the active turn");
    assert_eq!(closed, 0, "unsupported rewind closed the native writer");
    assert!(queued, "unsupported rewind withdrew accepted queued input");
    assert_eq!(snapshot["agent"]["activeTurn"]["turnId"], "native-turn");
}

#[tokio::test]
async fn dynamic_controls_are_consistent_across_creation_get_list_and_configuration() {
    let harness = Harness::new().await;
    let first = harness.snapshot().await;
    let listed = request(&harness.execution, "agent.list.request", json!({})).await;
    let configured = request(
        &harness.execution,
        "agent.model.set.request",
        json!({
            "agentId":harness.id, "modelId":"fast"
        }),
    )
    .await;
    let changed = harness.snapshot().await;
    harness.execution.shutdown().await.unwrap();
    assert_eq!(configured["accepted"], true);
    for field in ["features", "availableModes"] {
        assert_eq!(
            first["agent"][field], harness.created["agent"][field],
            "get lost {field}"
        );
        assert_eq!(
            listed["entries"][0]["agent"][field], harness.created["agent"][field],
            "list lost {field}"
        );
    }
    assert_eq!(changed["agent"]["features"][0]["value"], true);
}

#[tokio::test]
async fn reading_live_child_history_preserves_streamed_rows_and_cursors() {
    let harness = Harness::new().await;
    harness.send().await;
    // A session-scoped request drains all queued native observations before it replies.
    request(
        &harness.execution,
        "agent.model.set.request",
        json!({"agentId":harness.id,"modelId":"fast"}),
    )
    .await;
    let scope = format!("subagent:{}:native-child", harness.id);
    let timeline = harness.execution.timeline();
    let before = timeline.read(&scope).unwrap();
    assert!(!before.1.is_empty());
    let page = request(
        &harness.execution,
        "agent.provider_subagents.timeline.get.request",
        json!({
            "parentAgentId":harness.id, "subagentId":"native-child"
        }),
    )
    .await;
    let continuation = request(
        &harness.execution,
        "agent.provider_subagents.timeline.get.request",
        json!({
            "parentAgentId":harness.id, "subagentId":"native-child", "cursor":page["endCursor"]
        }),
    )
    .await;
    let after = timeline.read(&scope).unwrap();
    let history_reads = harness.state.lock().unwrap().history_reads;
    harness.execution.shutdown().await.unwrap();
    assert_eq!(
        after, before,
        "a read replaced a live child's committed projection"
    );
    assert_eq!(
        history_reads, 0,
        "a live projection must not wait for native disk history"
    );
    assert_eq!(continuation["rows"], json!([]));
    assert_eq!(page["rows"][0]["item"]["text"], "Live child output");
}
