use super::*;

#[tokio::test]
async fn max_and_ultra_create_retry_and_apply_to_following_turns() {
    for (effort, next_effort) in [("max", "ultra"), ("ultra", "max")] {
        let fixture = Fixture::new();
        fixture.mode("workflows");
        let (execution, registry) = worker(&fixture);
        let request = json!({
            "idempotencyKey":format!("reasoning-{effort}"),
            "config":{
                "provider":"codex","cwd":fixture.cwd,"model":"offline-model",
                "modeId":"auto-review","thinkingOptionId":effort,
                "featureValues":{"fast_mode":false}
            },
            "initialPrompt":"hello"
        });
        let created = execution
            .execute("agent.create.request", request.clone())
            .await
            .unwrap();
        let id = created["agentId"].as_str().unwrap();
        let finished = execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(finished["status"], "idle");
        assert_eq!(finished["lastMessage"], "Echo: hello");
        assert_eq!(
            registry
                .get(id)
                .unwrap()
                .unwrap()
                .runtime_info
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some(effort)
        );
        let retried = execution
            .execute("agent.create.request", request)
            .await
            .unwrap();
        assert_eq!(retried["agentId"], created["agentId"]);
        let configured = execution
            .execute(
                "agent.thinking.set.request",
                json!({"agentId":id,"thinkingOptionId":next_effort}),
            )
            .await
            .unwrap();
        assert_eq!(configured["accepted"], true);
        let sent = execution
            .execute(
                "agent.message.send.request",
                json!({"agentId":id,"text":"updated"}),
            )
            .await
            .unwrap();
        assert_eq!(sent["accepted"], true);
        let finished = execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(finished["lastMessage"], "Echo: updated");
        execution.shutdown().await.unwrap();

        let requests = fixture.requests();
        let start = requests
            .iter()
            .find(|request| request["method"] == "thread/start")
            .unwrap();
        assert_eq!(start["params"]["config"]["model_reasoning_effort"], effort);
        let turns: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .collect();
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["params"]["effort"], effort);
        assert_eq!(turns[1]["params"]["effort"], next_effort);
    }
}
