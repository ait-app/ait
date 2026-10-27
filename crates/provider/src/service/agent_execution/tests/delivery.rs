use super::*;

#[tokio::test]
async fn queued_admission_loads_canonical_history_from_newest_to_oldest_pages() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution.shutdown().await.unwrap();
    let pages = json!({
        "first": {
            "data": [{"type":"item","position":2,"turnId":"previous-turn",
                "item":{"type":"agentMessage","id":"newer","text":"Newer reply"}}],
            "nextCursor":"older"
        },
        "older": {
            "data": [{"type":"item","position":1,"turnId":"previous-turn",
                "item":{"type":"agentMessage","id":"older","text":"Older reply"}}],
            "nextCursor":null
        }
    });
    std::fs::write(fixture.cwd.join("timeline-pages.json"), pages.to_string()).unwrap();
    let (restarted, registry) = worker(&fixture);
    let sent = restarted
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"after history","messageId":"after-history"}),
        )
        .await
        .unwrap();
    let finished = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    let error = registry.get(id).unwrap().unwrap().last_error;
    restarted.shutdown().await.unwrap();
    assert_eq!(sent["accepted"], true, "{error:?}");
    assert_eq!(finished["lastMessage"], "Echo: after history", "{error:?}");
    assert_eq!(error, None);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
}

#[tokio::test]
async fn completion_racing_interrupt_accepts_replacement_without_a_false_failure() {
    let fixture = Fixture::new();
    fixture.mode("interrupt-completed");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang","messageId":"first"}),
        )
        .await
        .unwrap();
    let replacement = execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"replacement","messageId":"second"}),
        )
        .await
        .unwrap();
    assert_eq!(replacement["accepted"], true);
    let finished = execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: replacement");
    let entries = execution.timeline().read(id).unwrap().1;
    assert!(
        entries
            .iter()
            .any(|row| row.entry.item["text"] == "Echo: raced")
    );
    assert!(!entries.iter().any(|row| row.entry.item["type"] == "error"));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupt_delivery_drains_new_work_and_retries_are_durable_and_payload_bound() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let send = json!({"agentId":id,"text":"hang","messageId":"first"});
    assert_eq!(
        execution
            .execute("agent.message.send.request", send.clone())
            .await
            .unwrap()["accepted"],
        true
    );
    assert_eq!(
        execution
            .execute("agent.message.send.request", send)
            .await
            .unwrap()["accepted"],
        true
    );
    let replacement = json!({"agentId":id,"text":"replacement","messageId":"second"});
    assert_eq!(
        execution
            .execute("agent.message.send.request", replacement.clone())
            .await
            .unwrap()["accepted"],
        true
    );
    let finished = execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: replacement");
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r["method"] == "turn/start")
            .count(),
        2
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r["method"] == "turn/interrupt")
            .count(),
        1
    );
    execution.shutdown().await.unwrap();
    let (restarted, _) = worker(&fixture);
    assert_eq!(
        restarted
            .execute("agent.message.send.request", replacement)
            .await
            .unwrap()["accepted"],
        true
    );
    assert_eq!(
        restarted
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"changed","messageId":"second"})
            )
            .await
            .unwrap()["accepted"],
        false
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r["method"] == "turn/start")
            .count(),
        2
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn voice_exclusivity_blocks_interrupt_and_steer_without_reserving_or_delivering_input() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let accepted = json!({"agentId":id,"text":"completed before voice","messageId":"before-voice"});
    execution
        .execute("agent.message.send.request", accepted.clone())
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(
        execution
            .execute("internal.voice.send", json!({"agentId":id,"text":"hang"}))
            .await
            .unwrap()["accepted"],
        true
    );
    assert_eq!(
        execution
            .execute("agent.message.send.request", accepted)
            .await
            .unwrap()["accepted"],
        true
    );
    for behavior in ["interrupt", "steer"] {
        assert_eq!(
            execution
                .execute(
                    "agent.message.send.request",
                    json!({"agentId":id,"text":"blocked","activeTurnBehavior":behavior})
                )
                .await
                .unwrap()["accepted"],
            false
        );
    }
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|r| r["method"] == "turn/interrupt" || r["method"] == "turn/steer")
    );
    assert!(!execution.timeline().has_queued_input(id).unwrap());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_dispatches_only_unclaimed_fifo_inputs_and_keeps_uncertain_receipts_fenced() {
    use crate::protocol::prompt::AgentPrompt;
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let timeline = execution.timeline();
    execution.shutdown().await.unwrap();
    let prompt = AgentPrompt {
        client_message_id: Some("uncertain".into()),
        ..AgentPrompt::text("do not replay")
    };
    timeline
        .reserve_input(id, "uncertain", &prompt, "interrupt")
        .unwrap();
    timeline.claim_input(id, "uncertain").unwrap();
    timeline
        .reserve_input(id, "safe", &AgentPrompt::text("safe recovery"), "interrupt")
        .unwrap();
    let (restarted, _) = worker(&fixture);
    let finished = restarted
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: safe recovery");
    assert_eq!(
        restarted
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"do not replay","messageId":"uncertain"})
            )
            .await
            .unwrap()["accepted"],
        false
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r["method"] == "turn/start")
            .count(),
        1
    );
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_commands_are_idempotent_and_do_not_interrupt_a_busy_foreground_turn() {
    let fixture = Fixture::new();
    fixture.mode("workflows");
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    for (message, text) in [("goal-pause", "/goal pause"), ("compact-one", "/compact")] {
        let request = json!({"agentId":id,"text":text,"messageId":message});
        assert_eq!(
            execution
                .execute("agent.message.send.request", request.clone())
                .await
                .unwrap()["accepted"],
            true
        );
        assert_eq!(
            execution
                .execute("agent.message.send.request", request)
                .await
                .unwrap()["accepted"],
            true
        );
    }
    let requests = fixture.requests();
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "turn/interrupt")
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "thread/goal/set")
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "thread/compact/start")
            .count(),
        1
    );
    assert!(requests.iter().any(|request| {
        request["method"] == "thread/goal/set"
            && request["argv"]
                .as_array()
                .unwrap()
                .contains(&json!("goals"))
    }));
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
    let (execution, _) = worker(&fixture);
    let history = execution
        .execute("agent.timeline.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(
        history["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["item"]["text"] == "Goal paused.")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_goal_continuations_acquire_and_release_the_host_foreground_turn() {
    let fixture = Fixture::new();
    fixture.mode("workflows");
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(
        execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"/goal autonomous fixture"})
            )
            .await
            .unwrap()["accepted"],
        true
    );
    let finished = execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: Goal completed");
    assert_eq!(
        registry.get(id).unwrap().unwrap().last_status,
        domain::agent_runtime::AgentRuntimeStatus::Idle
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_queued_admission_does_not_block_an_independent_agent() {
    use crate::protocol::prompt::AgentPrompt;
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let first = create(&execution, &fixture).await;
    let second = create(&execution, &fixture).await;
    let first = first["agentId"].as_str().unwrap();
    let second = second["agentId"].as_str().unwrap();
    let timeline = execution.timeline();
    execution.shutdown().await.unwrap();
    timeline
        .reserve_input(
            first,
            "rejected-input",
            &AgentPrompt::text("reject-before-admission"),
            "interrupt",
        )
        .unwrap();
    timeline
        .reserve_input(
            second,
            "valid-input",
            &AgentPrompt::text("independent input"),
            "interrupt",
        )
        .unwrap();
    let (restarted, registry) = worker(&fixture);
    let finished = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":second,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: independent input");
    // Independent lanes can finish in either order; observe the failed admission separately.
    let rejected = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":first,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    assert_eq!(rejected["status"], "error");
    let failed = registry.get(first).unwrap().unwrap();
    assert_eq!(
        failed.last_status,
        domain::agent_runtime::AgentRuntimeStatus::Error
    );
    assert!(failed.requires_attention);
    assert!(!timeline.has_queued_input(first).unwrap());
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn restart_reloads_large_completed_and_interrupted_histories_before_one_native_turn() {
    for status in ["completed", "interrupted"] {
        let fixture = Fixture::new();
        let (execution, registry) = worker(&fixture);
        let created = create(&execution, &fixture).await;
        let id = created["agentId"].as_str().unwrap();
        let native_id = registry
            .get(id)
            .unwrap()
            .unwrap()
            .persistence
            .unwrap()
            .session_id;
        execution.shutdown().await.unwrap();
        let original = json!([{"id":"previous-turn","status":status,"items":[{
            "id":"original","type":"agentMessage","text":"Original conversation"}]}]);
        let path = fixture.cwd.join(format!("native-history-{native_id}.json"));
        std::fs::write(&path, original.to_string()).unwrap();
        assert!(
            crate::test_support::codex_history::write_large_history(&fixture) > 32 * 1024 * 1024
        );
        let (restarted, registry) = worker(&fixture);
        let prompt = json!({"agentId":id,"text":"Continue","messageId":"after-restart"});
        assert_eq!(
            restarted
                .execute("agent.message.send.request", prompt.clone())
                .await
                .unwrap()["accepted"],
            true
        );
        let finished = restarted
            .execute(
                "agent.finish.wait.request",
                json!({"agentId":id,"timeoutMs":5000}),
            )
            .await
            .unwrap();
        assert_eq!(finished["lastMessage"], "Echo: Continue");
        assert_eq!(registry.get(id).unwrap().unwrap().last_error, None);
        assert_eq!(
            restarted
                .execute("agent.message.send.request", prompt)
                .await
                .unwrap()["accepted"],
            true
        );
        restarted.shutdown().await.unwrap();
        let requests = fixture.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "turn/start")
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .filter(|request| request["method"] == "thread/resume")
                .all(|request| request["params"]["threadId"] == native_id)
        );
        assert!(
            requests
                .iter()
                .filter(|request| request["method"] == "thread/read")
                .all(|request| request["params"]["includeTurns"] == false)
        );
        let turns: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(turns.as_array().unwrap().len(), 2);
        assert_eq!(turns[0], original[0]);
    }
}

#[tokio::test]
async fn failed_history_admission_preserves_diagnostics_and_never_replays_uncertain_input() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution.shutdown().await.unwrap();
    let path = fixture.cwd.join("timeline-pages.json");
    std::fs::write(
        &path,
        json!({"first":{"data":[{"position":"invalid"}],"nextCursor":null}}).to_string(),
    )
    .unwrap();
    let (restarted, registry) = worker(&fixture);
    let prompt = json!({"agentId":id,"text":"Continue","messageId":"failed-history"});
    assert_eq!(
        restarted
            .execute("agent.message.send.request", prompt.clone())
            .await
            .unwrap()["accepted"],
        false
    );
    let error = registry.get(id).unwrap().unwrap().last_error.unwrap();
    assert!(error.contains("thread/timeline/list, page 1"), "{error}");
    assert!(error.contains("invalid history protocol"), "{error}");
    assert!(!error.contains("registry"), "{error}");
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    std::fs::write(
        &path,
        json!({"first":{"data":[],"nextCursor":null}}).to_string(),
    )
    .unwrap();
    // Failed admission is conservatively fenced. Reusing its key cannot create a duplicate turn.
    assert_ne!(
        restarted
            .execute("agent.message.send.request", prompt)
            .await
            .ok()
            .map(|value| value["accepted"].clone()),
        Some(json!(true))
    );
    let sent = restarted
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"Continue","messageId":"after-recovery"}),
        )
        .await
        .unwrap();
    assert_eq!(sent["accepted"], true);
    let finished = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":5000}),
        )
        .await
        .unwrap();
    assert_eq!(finished["lastMessage"], "Echo: Continue");
    assert_eq!(registry.get(id).unwrap().unwrap().last_error, None);
    restarted.shutdown().await.unwrap();
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
}
