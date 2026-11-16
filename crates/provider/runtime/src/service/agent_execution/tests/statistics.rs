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
    let reads = (0..3).map(|_| {
        tokio::time::timeout(
            Duration::from_secs(2),
            execution.execute("agent.statistics.get.request", json!({"agentId":id})),
        )
    });
    for result in futures_util::future::join_all(reads).await {
        let stats = result
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
async fn statistics_observed_child_reads_only_its_committed_counters() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let root_id = created["agentId"].as_str().unwrap();
    execution.shutdown().await.unwrap();
    let mut root = registry.get(root_id).unwrap().unwrap();
    root.runtime_info
        .as_mut()
        .unwrap()
        .extra
        .get_or_insert_with(Default::default)
        .insert("lastUsage".into(), json!({"inputTokens":100}));
    registry.upsert(&root).unwrap();
    let mut child = root.clone();
    child.id = uuid::Uuid::new_v4().to_string();
    child.persistence = None;
    child
        .labels
        .insert("paseo.parent-agent-id".into(), root_id.into());
    child
        .runtime_info
        .as_mut()
        .unwrap()
        .extra
        .as_mut()
        .unwrap()
        .insert(
            "lastUsage".into(),
            json!({"inputTokens":9,"outputTokens":0}),
        );
    child.observation = Some(
        serde_json::from_value(json!({
            "rootAgentId":root_id,"rootSessionId":"native-root","parentAgentId":root_id,
            "nativeId":"native-child","parentNativeId":"native-root","persistence":null,
            "ready":true,"status":"running","health":"current","observedAt":child.updated_at,
            "description":null,"subtitle":null
        }))
        .unwrap(),
    );
    registry.upsert(&child).unwrap();
    let requests = fixture.requests();
    let (restarted, _) = worker(&fixture);
    let statistics = restarted
        .execute("agent.statistics.get.request", json!({"agentId":child.id}))
        .await
        .unwrap();
    assert_eq!(statistics["agentId"], child.id);
    assert_eq!(statistics["usage"]["inputTokens"], 9);
    assert_eq!(statistics["usage"]["outputTokens"], 0);
    assert_eq!(registry.get(root_id).unwrap().unwrap(), root);
    assert_eq!(registry.get(&child.id).unwrap().unwrap(), child);
    assert_eq!(fixture.requests(), requests);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn statistics_hidden_bot_inspection_uses_exact_identity_without_restoring_a_provider() {
    use crate::service::bot_execution::Action;
    use domain::bot::{ExecutionConfig, SessionOwner};
    use domain::{BotId, ConversationId};

    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let owner = SessionOwner {
        bot_id: BotId::generate(),
        conversation_id: ConversationId::generate(),
        workspace_id: "wks_0123456789abcdef".into(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    execution
        .bot(
            &owner,
            Action::Create {
                agent_id: id.clone(),
                config: ExecutionConfig {
                    provider: "codex".into(),
                    model: None,
                    mode_id: None,
                    thinking: None,
                },
                instructions: "Bot statistics test".into(),
            },
        )
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
    let saved = registry.get(&id).unwrap().unwrap();
    assert!(saved.internal);
    let requests = fixture.requests();
    let (restarted, _) = worker(&fixture);
    let statistics = restarted
        .execute("agent.statistics.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(statistics["agentId"], id);
    assert_eq!(statistics["provider"], "codex");
    assert_eq!(
        restarted
            .execute("agent.statistics.get.request", json!({"agentId":&id[..8]}),)
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    assert_eq!(registry.get(&id).unwrap().unwrap(), saved);
    assert_eq!(fixture.requests(), requests);
    restarted.shutdown().await.unwrap();
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
