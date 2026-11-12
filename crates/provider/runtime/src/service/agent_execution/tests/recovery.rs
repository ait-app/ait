use super::*;

async fn interrupted_record(
    fixture: &Fixture,
) -> (String, domain::agent_runtime::AgentPersistenceHandle) {
    let (execution, registry) = worker(fixture);
    let created = create(&execution, fixture).await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang","messageId":"original"}),
        )
        .await
        .unwrap();
    let running = registry.get(&id).unwrap().unwrap();
    assert_eq!(
        running.last_status,
        domain::agent_runtime::AgentRuntimeStatus::Running
    );
    let handle = running.persistence.clone().unwrap();
    execution.shutdown().await.unwrap();
    // Preserve the durable snapshot left by a host killed before its terminal transaction.
    registry.upsert(&running).unwrap();
    (id, handle)
}

#[tokio::test]
async fn restart_continues_the_original_codex_session_without_replaying_its_prompt() {
    let fixture = Fixture::new();
    let (id, handle) = interrupted_record(&fixture).await;

    let (restarted, registry) = worker(&fixture);
    let finished = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":10000}),
        )
        .await
        .unwrap();

    assert_eq!(finished["status"], "idle", "{finished}");
    assert!(
        finished["lastMessage"]
            .as_str()
            .unwrap()
            .contains("host restarted unexpectedly")
    );
    assert!(registry.get(&id).unwrap().unwrap().last_error.is_none());
    assert_eq!(
        registry.get(&id).unwrap().unwrap().persistence,
        Some(handle.clone())
    );
    let requests = fixture.requests();
    let turns: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "turn/start")
        .collect();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0]["params"]["input"][0]["text"], "hang");
    assert!(
        turns[1]["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Verify any unfinished tool operation")
    );
    assert_eq!(turns[1]["params"]["threadId"], handle.session_id);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_continuation_preserves_the_native_error_and_allows_a_later_retry() {
    let fixture = Fixture::new();
    let (id, handle) = interrupted_record(&fixture).await;
    fixture.mode("reject-first-input");
    let (restarted, registry) = worker(&fixture);

    let failed = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":10000}),
        )
        .await
        .unwrap();

    assert_eq!(failed["status"], "error", "{failed}");
    let message = registry.get(&id).unwrap().unwrap().last_error.unwrap();
    assert!(message.contains("sensitive native error"), "{message}");
    assert_eq!(failed["error"], message);
    assert!(!restarted.timeline().has_queued_input(&id).unwrap());
    let errors: Vec<_> = restarted
        .timeline()
        .read(&id)
        .unwrap()
        .1
        .into_iter()
        .filter(|row| row.entry.item["type"] == "error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].entry.item["message"], message);
    restarted.shutdown().await.unwrap();
    fixture.mode("normal");
    let (retried, registry) = worker(&fixture);
    retried
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"Continue","messageId":"explicit-retry"}),
        )
        .await
        .unwrap();
    let finished = retried
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":10000}),
        )
        .await
        .unwrap();
    assert_eq!(finished["status"], "idle", "{finished}");
    assert_eq!(finished["lastMessage"], "Echo: Continue");
    assert_eq!(
        registry.get(&id).unwrap().unwrap().persistence,
        Some(handle)
    );
    retried.shutdown().await.unwrap();
}

#[tokio::test]
async fn unavailable_provider_finishes_recovery_with_a_durable_visible_error() {
    let fixture = Fixture::new();
    let (id, _) = interrupted_record(&fixture).await;
    std::fs::remove_file(&fixture.program).unwrap();
    let (restarted, registry) = worker(&fixture);

    let failed = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":10000}),
        )
        .await
        .unwrap();

    assert_eq!(failed["status"], "error", "{failed}");
    let message = registry.get(&id).unwrap().unwrap().last_error.unwrap();
    assert!(message.contains("host restarted"), "{message}");
    assert!(message.contains("provider unavailable"), "{message}");
    let errors: Vec<_> = restarted
        .timeline()
        .read(&id)
        .unwrap()
        .1
        .into_iter()
        .filter(|row| row.entry.item["type"] == "error")
        .collect();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].entry.item["message"], message);
    restarted.shutdown().await.unwrap();
}

#[tokio::test]
async fn completion_wait_stays_pending_while_restart_resume_is_being_admitted() {
    let fixture = Fixture::new();
    let (id, _) = interrupted_record(&fixture).await;
    fixture.mode("delayed-resume");
    let (restarted, _) = worker(&fixture);
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture.cwd.join("resume-started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();

    let pending = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":1}),
        )
        .await
        .unwrap();
    std::fs::write(fixture.cwd.join("release-resume"), "release").unwrap();
    assert_eq!(pending["status"], "timeout", "{pending}");
    assert!(pending["error"].is_null());
    let completed = restarted
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":10000}),
        )
        .await
        .unwrap();
    assert_eq!(completed["status"], "idle", "{completed}");
    restarted.shutdown().await.unwrap();
}
