//! Paseo resumeAgentFromPersistence settings and rollback contracts through native RPC.

use super::*;

#[tokio::test]
async fn unknown_native_handle_uses_metadata_and_overrides_without_creating_provider_history() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let old_id = created["agentId"].as_str().unwrap();
    execution
        .execute("agent.delete.request", json!({"agentId":old_id}))
        .await
        .unwrap();
    let mut handle = created["agent"]["persistence"].clone();
    handle["metadata"] =
        json!({"cwd":fixture.cwd,"model":"metadata-model","systemPrompt":"metadata prompt"});
    let response = execution
        .execute(
            "agent.resume.request",
            json!({"handle":handle,
        "overrides":{"model":"offline-model","title":"Native restore"}}),
        )
        .await
        .unwrap();
    let id = response["agentId"].as_str().unwrap();
    assert_ne!(id, old_id);
    assert!(registry.get(old_id).unwrap().is_none());
    assert_eq!(registry.list().unwrap().len(), 1);
    assert_eq!(response["agent"]["title"], "Native restore");
    let record = registry.get(id).unwrap().unwrap();
    assert_eq!(
        record.config.as_ref().unwrap().system_prompt.as_deref(),
        Some("metadata prompt")
    );
    assert_eq!(
        record.config.as_ref().unwrap().model.as_deref(),
        Some("offline-model")
    );
    assert_eq!(
        record.persistence.as_ref().unwrap().session_id,
        handle["sessionId"]
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "thread/start")
            .count(),
        1
    );
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"restored native history"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_unknown_resume_does_not_create_agent_records_or_native_writers() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    for params in [
        json!({"handle":{"provider":"codex","sessionId":"external"}}),
        json!({"handle":{"provider":"codex","sessionId":"external","metadata":{"cwd":fixture.cwd}},"overrides":{"provider":"claude"}}),
        json!({"handle":{"provider":"codex","sessionId":"external","metadata":{"cwd":"/missing"}}}),
    ] {
        assert_eq!(
            execution.execute("agent.resume.request", params).await,
            Err(ErrorCode::InvalidMessage)
        );
    }
    fixture.mode("missing-thread");
    assert!(execution.execute("agent.resume.request", json!({"handle":{"provider":"codex","sessionId":"external"},"overrides":{"cwd":fixture.cwd}})).await.is_err());
    assert_eq!(
        registry.list().unwrap(),
        [] as [domain::agent_runtime::PersistedAgentRuntimeRecord; 0]
    );
    assert!(!fixture.requests().iter().any(|request| matches!(
        request["method"].as_str(),
        Some("thread/start" | "thread/resume")
    )));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_cwd_override_keeps_workspace_identity_and_remains_sendable() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let child = fixture.cwd.join("child");
    std::fs::create_dir(&child).unwrap();
    let original = registry.get(id).unwrap().unwrap();
    execution
        .execute(
            "agent.resume.request",
            json!({"handle":created["agent"]["persistence"],"overrides":{"cwd":child.join(".")}}),
        )
        .await
        .unwrap();
    let record = registry.get(id).unwrap().unwrap();
    assert_eq!(record.workspace_id, original.workspace_id);
    assert_eq!(record.cwd, child.canonicalize().unwrap().to_str().unwrap());
    let sent = execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"from changed cwd"}),
        )
        .await
        .unwrap();
    assert_eq!(sent["accepted"], true, "{sent}");
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn resume_overrides_reach_the_native_session_and_survive_restart() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.config.apply.request",
            json!({"agentId":id,
        "config":{"modelId":"initial-model","systemPrompt":"preserve prompt","thinkingOptionId":"high"}}),
        )
        .await
        .unwrap();
    let response = execution.execute("agent.resume.request", json!({"handle":created["agent"]["persistence"],
        "overrides":{"model":"offline-model","title":"  Restored title  ","featureValues":{"fast_mode":true}}})).await.unwrap();
    assert_eq!(response["status"], "agent_resumed");
    assert!(response["timelineSize"].is_number());
    assert_eq!(response["agent"]["title"], "Restored title");
    let record = registry.get(id).unwrap().unwrap();
    let config = record.config.as_ref().unwrap();
    assert_eq!(config.system_prompt.as_deref(), Some("preserve prompt"));
    assert_eq!(config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(config.model.as_deref(), Some("offline-model"));
    let requests = fixture.requests();
    let native = requests
        .iter()
        .rev()
        .find(|request| request["method"] == "thread/resume")
        .unwrap();
    assert_eq!(native["params"]["model"], "offline-model");
    assert_eq!(native["params"]["config"]["model_reasoning_effort"], "high");
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"after restore"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    let requests = fixture.requests();
    let turn = requests
        .iter()
        .rev()
        .find(|request| request["method"] == "turn/start")
        .unwrap();
    assert_eq!(turn["params"]["serviceTier"], "fast");
    execution.shutdown().await.unwrap();
    let (restarted, reopened_registry) = worker(&fixture);
    let reopened = reopened_registry.get(id).unwrap().unwrap();
    assert_eq!(reopened.config, record.config);
    assert_eq!(reopened.title, record.title);
    restarted
        .execute(
            "agent.resume.request",
            json!({"handle":created["agent"]["persistence"],"overrides":{"title":null}}),
        )
        .await
        .unwrap();
    restarted.shutdown().await.unwrap();
    assert_ne!(
        reopened_registry.get(id).unwrap().unwrap().title,
        Some("Restored title".to_owned())
    );
}

#[tokio::test]
async fn failed_native_restore_keeps_the_agent_archived_and_its_previous_settings() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute("agent.archive.request", json!({"agentId":id}))
        .await
        .unwrap();
    let before = registry.get(id).unwrap().unwrap();
    fixture.mode("missing-thread");
    assert!(
        execution
            .execute(
                "agent.resume.request",
                json!({"handle":created["agent"]["persistence"],
        "overrides":{"title":"Do not persist","model":"next-model"}})
            )
            .await
            .is_err()
    );
    let after = registry.get(id).unwrap().unwrap();
    assert_eq!(after.archived_at, before.archived_at);
    assert_eq!(after.config, before.config);
    assert_eq!(after.title, before.title);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn invalid_restore_does_not_close_the_live_session_or_discard_an_active_turn() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let before = registry.get(id).unwrap().unwrap();
    for overrides in [
        json!({"title":" "}),
        json!({"provider":"claude"}),
        json!({"model":""}),
        json!({"thinkingOptionId":"imaginary"}),
        json!({"unknown":true}),
    ] {
        assert!(
            execution
                .execute(
                    "agent.resume.request",
                    json!({"handle":created["agent"]["persistence"],"overrides":overrides})
                )
                .await
                .is_err()
        );
        assert_eq!(registry.get(id).unwrap().unwrap(), before);
    }
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    assert!(
        execution
            .execute(
                "agent.resume.request",
                json!({"handle":created["agent"]["persistence"],"overrides":{"model":"next-model"}})
            )
            .await
            .is_err()
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/resume")
    );
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution.shutdown().await.unwrap();
}
