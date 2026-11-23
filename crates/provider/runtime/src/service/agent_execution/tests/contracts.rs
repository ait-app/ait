//! Public execution contracts with a deterministic native adapter and isolated durable storage.

use serde_json::json;

use crate::testing::request;
use provider_core::protocol::prompt::AgentPrompt;

mod fixture;
use fixture::Harness;

#[tokio::test]
async fn child_control_wait_does_not_block_parent_permission_and_returns_native_outcome() {
    let harness = Harness::new().await;
    harness.state.lock().unwrap().control_test = true;
    harness.send().await;
    let listed = request(
        &harness.execution,
        "agent.subagents.list.request",
        json!({"parentAgentId":harness.id}),
    )
    .await;
    let child = listed["agents"][0]["id"]
        .as_str()
        .expect("observed child")
        .to_owned();
    let revision = listed["agents"][0]["observation"]["revision"]
        .as_u64()
        .expect("revision");
    let executor = harness.execution.clone();
    let parent = harness.id.clone();
    let child_id = child.clone();
    let control = tokio::spawn(async move {
        executor.execute("agent.subagents.guide.request", json!({"parentAgentId":parent,"childAgentId":child_id,"message":"  guide\n","expectedRevision":revision})).await
    });
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while harness.state.lock().unwrap().child_controls.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("control admitted");
    assert!(!control.is_finished(), "native receipt is still pending");
    tokio::time::timeout(std::time::Duration::from_secs(2), harness.execution.execute(
        "agent.permission.resolve.request",
        json!({"agentId":harness.id,"requestId":"permission-1","response":{"behavior":"allow"}}),
    )).await.expect("permission must not wait for child control").unwrap();
    assert_eq!(harness.state.lock().unwrap().permission_answers, 1);
    harness.state.lock().unwrap().release_control = true;
    let receipt = tokio::time::timeout(std::time::Duration::from_secs(3), control)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(receipt["disposition"], "accepted");
    assert_eq!(receipt["childAgentId"], child);
    assert_eq!(
        harness.state.lock().unwrap().child_controls,
        [("native-child".to_owned(), "  guide\n".to_owned())]
    );
    harness.execution.shutdown().await.unwrap();
}

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

#[tokio::test]
async fn attention_clear_accepts_the_documented_agent_id_array() {
    let harness = Harness::new().await;
    let result = harness
        .execution
        .execute(
            "agent.attention.clear.request",
            json!({"agentId":[harness.id.clone()]}),
        )
        .await;
    harness.execution.shutdown().await.unwrap();
    assert!(result.is_ok(), "{result:?}");
}
