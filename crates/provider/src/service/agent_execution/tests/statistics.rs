use super::*;

#[tokio::test]
async fn statistics_read_during_active_turn_without_provider_input_or_interruption() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    let before = registry.get(id).unwrap().unwrap();
    let requests = fixture.requests();
    for _ in 0..3 {
        let stats = tokio::time::timeout(
            Duration::from_secs(2),
            execution.execute("agent.statistics.get.request", json!({"agentId":id})),
        )
        .await
        .expect("statistics must not wait for the active native turn")
        .unwrap();
        assert_eq!(stats["agentId"], id);
        assert_eq!(stats["status"], "running");
        assert_eq!(stats["usageScope"], "provider_snapshot");
    }
    assert_eq!(fixture.requests(), requests, "read issued a native request");
    assert_eq!(
        registry.get(id).unwrap().unwrap(),
        before,
        "read mutated the Agent"
    );
    assert_eq!(
        execution
            .execute(
                "agent.finish.wait.request",
                json!({"agentId":id,"timeoutMs":1})
            )
            .await
            .unwrap()["status"],
        "timeout"
    );
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn statistics_restart_reads_persisted_usage_without_restoring_provider() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution.shutdown().await.unwrap();
    registry
        .update(id, &|record| {
            let mut record = record.clone();
            record
                .runtime_info
                .as_mut()
                .unwrap()
                .extra
                .get_or_insert_with(Default::default)
                .insert(
                    "lastUsage".to_owned(),
                    json!({"inputTokens":900,"outputTokens":0}),
                );
            record
        })
        .unwrap();
    let (restarted, _) = worker(&fixture);
    let requests = fixture.requests();
    for _ in 0..2 {
        let stats = restarted
            .execute("agent.statistics.get.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(stats["usage"]["inputTokens"], 900);
        assert_eq!(stats["usage"]["outputTokens"], 0);
        assert_eq!(stats["usage"]["totalCostUsd"], Value::Null);
    }
    assert_eq!(fixture.requests(), requests);
    assert_eq!(
        restarted
            .execute("agent.statistics.get.request", json!({"agentId":"missing"}))
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    restarted.shutdown().await.unwrap();
}
