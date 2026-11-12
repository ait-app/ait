//! Ported create-agent/intent.test.ts ownership and caller precedence cases.

use super::*;

#[tokio::test]
async fn caller_inherits_placement_and_parentage_without_using_the_stale_draft_cwd() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let parent = create(&execution, &fixture).await;
    let child = execution
        .execute(
            "agent.create.request",
            json!({"callerAgentId":parent["agentId"],
        "config":{"provider":"codex","cwd":"/missing/stale/draft"},
        "labels":{"purpose":"review","paseo.parent-agent-id":"spoofed"}}),
        )
        .await
        .unwrap();
    let record = registry
        .get(child["agentId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        record.workspace_id.as_deref(),
        parent["agent"]["workspaceId"].as_str()
    );
    assert_eq!(
        record.cwd,
        fixture.cwd.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        record.labels["paseo.parent-agent-id"],
        parent["agentId"].as_str().unwrap()
    );
    assert_eq!(record.labels["purpose"], "review");
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_workspace_overrides_requested_cwd_and_preserves_caller_parentage() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let parent = create(&execution, &fixture).await;
    let child = execution.execute("agent.create.request", json!({"callerAgentId":parent["agentId"],
        "workspaceId":"wks_0123456789abcdef", "config":{"provider":"codex","cwd":"does-not-exist"}})).await.unwrap();
    let record = registry
        .get(child["agentId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        record.cwd,
        fixture.cwd.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(record.workspace_id.as_deref(), Some("wks_0123456789abcdef"));
    assert_eq!(
        record.labels["paseo.parent-agent-id"],
        parent["agentId"].as_str().unwrap()
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_caller_is_rejected_before_any_native_session_is_created() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    assert_eq!(
        execution
            .execute(
                "agent.create.request",
                json!({"callerAgentId":"missing",
        "config":{"provider":"codex","cwd":fixture.cwd}})
            )
            .await,
        Err(ErrorCode::AgentNotFound)
    );
    assert_eq!(
        registry.list().unwrap(),
        [] as [domain::agent_runtime::PersistedAgentRuntimeRecord; 0]
    );
    assert!(!fixture.cwd.join("native-requests.jsonl").exists());
    execution.shutdown().await.unwrap();
}
