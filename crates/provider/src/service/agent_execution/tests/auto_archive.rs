use super::*;

#[tokio::test]
async fn auto_archive_request_waits_for_a_terminal_event_and_removes_the_active_directory_entry() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"codex","cwd":fixture.cwd},"autoArchive":true}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap();
    assert!(registry.get(id).unwrap().unwrap().archived_at.is_none());
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    assert_eq!(
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap()["status"],
        "permission"
    );
    assert!(registry.get(id).unwrap().unwrap().archived_at.is_none());
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(registry.get(id).unwrap().unwrap().archived_at.is_some());
    let listed = execution
        .execute("agent.list.request", json!({}))
        .await
        .unwrap();
    assert_eq!(listed["entries"], serde_json::json!([]));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn auto_archive_initial_prompt_runs_to_completion_and_false_keeps_the_agent_active() {
    for enabled in [false, true] {
        let fixture = Fixture::new();
        let (execution, registry) = worker(&fixture);
        let created = execution.execute("agent.create.request", json!({"config":{"provider":"codex","cwd":fixture.cwd},"initialPrompt":"hello","autoArchive":enabled})).await.unwrap();
        let id = created["agentId"].as_str().unwrap();
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap();
        assert_eq!(
            registry.get(id).unwrap().unwrap().archived_at.is_some(),
            enabled
        );
        execution.shutdown().await.unwrap();
    }
}
