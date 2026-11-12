//! Wait responses adapted from Paseo session wait-for-finish and Agent manager tests.

use super::*;

#[tokio::test]
async fn unknown_and_internal_agents_return_inline_wait_errors() {
    let fixture = Fixture::new();
    let (execution, registry) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    registry
        .update(id, &|record| {
            let mut next = record.clone();
            next.internal = true;
            next
        })
        .unwrap();
    for target in ["does-not-exist", id] {
        let result = execution
            .execute("agent.finish.wait.request", json!({"agentId":target}))
            .await
            .unwrap();
        assert_eq!(result["status"], "error");
        assert!(result["final"].is_null());
        assert!(result["lastMessage"].is_null());
        assert!(result["error"].is_string());
    }
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn in_flight_wait_keeps_final_text_after_auto_archive_but_later_wait_is_stored_only() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = execution
        .execute(
            "agent.create.request",
            json!({"config":{"provider":"codex","cwd":fixture.cwd},
                "initialPrompt":"hang","autoArchive":true}),
        )
        .await
        .unwrap();
    let id = created["agentId"].as_str().unwrap().to_owned();
    let waiter = execution.clone();
    let target = id.clone();
    let waiting = tokio::spawn(async move {
        waiter
            .execute("agent.finish.wait.request", json!({"agentId":target}))
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiting.is_finished());
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"final answer","activeTurnBehavior":"steer"}),
        )
        .await
        .unwrap();
    let result = waiting.await.unwrap().unwrap();
    assert_eq!(result["status"], "idle");
    assert_eq!(result["lastMessage"], "Echo: final answer");
    assert!(result["final"]["archivedAt"].is_string());
    let later = execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(later["status"], "idle");
    assert!(later["lastMessage"].is_null());
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_permission_finishes_a_wait_without_canceling_the_native_turn() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    let first = execution
        .execute("agent.list.request", json!({"scope":"active","sync":{}}))
        .await
        .unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let permission = controls::pending(&execution, id).await;
    let synchronized = execution
        .execute(
            "agent.list.request",
            json!({"scope":"active","sync":{
                "generation":first["sync"]["generation"],"afterSeq":first["sync"]["headSeq"]
            }}),
        )
        .await
        .unwrap();
    let synced = synchronized["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["agent"]["id"] == id)
        .unwrap();
    assert_eq!(
        synced["agent"]["pendingPermissions"][0]["id"],
        permission["id"]
    );
    assert_eq!(synced["agent"]["providerUnavailable"], false);
    let waited = execution
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":100}),
        )
        .await
        .unwrap();
    assert_eq!(waited["status"], "permission", "{waited}");
    assert!(waited["error"].is_null());
    assert_eq!(
        waited["final"]["pendingPermissions"][0]["id"],
        permission["id"]
    );
    execution
        .execute(
            "agent.permission.resolve.request",
            json!({"agentId":id,"requestId":permission["id"],"response":{"behavior":"allow"}}),
        )
        .await
        .unwrap();
    assert_eq!(
        execution
            .execute("agent.finish.wait.request", json!({"agentId":id}))
            .await
            .unwrap()["status"],
        "idle"
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn waits_accept_longer_explicit_timeouts_and_omit_stale_messages_on_timeout() {
    let fixture = Fixture::new();
    let (execution, _) = worker(&fixture);
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"finished"}),
        )
        .await
        .unwrap();
    let finished = execution
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":60_000}),
        )
        .await
        .unwrap();
    assert_eq!(finished["status"], "idle");
    assert_eq!(finished["lastMessage"], "Echo: finished");
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang"}),
        )
        .await
        .unwrap();
    let timed_out = execution
        .execute(
            "agent.finish.wait.request",
            json!({"agentId":id,"timeoutMs":1}),
        )
        .await
        .unwrap();
    assert_eq!(timed_out["status"], "timeout");
    assert!(timed_out["lastMessage"].is_null());
    assert!(timed_out["error"].is_null());
    for timeout in [0, 9_007_199_254_740_992_u64] {
        assert_eq!(
            execution
                .execute(
                    "agent.finish.wait.request",
                    json!({"agentId":id,"timeoutMs":timeout})
                )
                .await,
            Err(ErrorCode::InvalidMessage)
        );
    }
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
