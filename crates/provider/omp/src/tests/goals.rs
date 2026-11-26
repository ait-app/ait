//! Goal admission through the same creation endpoint used by the composer.

use super::*;

#[tokio::test]
async fn initial_goal_uses_native_control_and_creation_retry_does_not_duplicate_it() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_GOAL", "1")]);
    let (execution, _) = provider::testing::worker_at(
        root.path(),
        std::path::Path::new(&spec.cwd),
        model::creation::Creations::default(),
        Box::new(client),
    );
    let request = json!({
        "config":{"provider":"omp","cwd":spec.cwd},
        "initialPrompt":"/goal Complete the acceptance criteria\nPreserve existing changes.",
        "clientMessageId":"initial-goal", "idempotencyKey":"goal-creation"
    });
    let created = execution
        .execute("agent.create.request", request.clone())
        .await
        .unwrap_or_else(|error| {
            panic!(
                "creation failed: {error:?}; calls: {:?}",
                calls_after(root.path(), 0)
            )
        });
    assert_eq!(created["status"], "agent_created");
    let retried = execution
        .execute("agent.create.request", request)
        .await
        .unwrap();
    assert_eq!(retried["agentId"], created["agentId"]);
    execution.shutdown().await.unwrap();
    let calls = calls_after(root.path(), 0);
    let goals: Vec<_> = calls.iter().filter(|call| call["type"] == "goal").collect();
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0]["op"], "create");
    assert_eq!(
        goals[0]["objective"],
        "Complete the acceptance criteria\nPreserve existing changes."
    );
    assert_eq!(count(&calls, "prompt"), 0);
}

#[tokio::test]
async fn pending_goal_can_be_stopped_before_its_first_turn_and_resumed() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_GOAL", "1")]);
    let mut session = client.create_session(&spec).await.unwrap();
    commands_ready(session.as_mut()).await;
    session
        .out_of_band(&AgentPrompt::text("/goal Ship"))
        .await
        .unwrap();
    assert!(session.pending_foreground());
    session.cancel_pending().await.unwrap();
    assert!(!session.pending_foreground());
    session.cancel_pending().await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["goal"]["status"],
        "paused"
    );
    // Drain the earlier create/pause notifications before a new user operation.
    while session.poll_turn().unwrap().is_some() {}
    session
        .out_of_band(&AgentPrompt::text("/goal resume"))
        .await
        .unwrap();
    assert!(session.pending_foreground());
    session
        .out_of_band(&AgentPrompt::text("/goal clear"))
        .await
        .unwrap();
    loop {
        if let AgentTurnEvent::RuntimeInfo(info) = next(session.as_mut()).await.unwrap()
            && info.extra.unwrap()["goal"].is_null()
        {
            break;
        }
    }
    assert!(!session.pending_foreground());
    session.close().await.unwrap();
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "abort"), 1);
    assert_eq!(count(&calls, "prompt"), 0);
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["type"] == "goal" && call["op"] == "pause")
            .count(),
        1
    );
}

#[tokio::test]
async fn a_rejected_goal_does_not_leave_foreground_admission_reserved() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_GOAL", "1")]);
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/goal.json"),
        json!({"objective":"Existing", "status":"paused"}).to_string(),
    )
    .unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    commands_ready(session.as_mut()).await;
    session
        .out_of_band(&AgentPrompt::text("/goal Another"))
        .await
        .unwrap();
    assert!(session.pending_foreground());
    loop {
        if let AgentTurnEvent::Timeline(entry) = next(session.as_mut()).await.unwrap()
            && entry.item["name"] == "goal"
        {
            assert!(
                entry.item["detail"]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Goal operation rejected")
            );
            break;
        }
    }
    assert!(!session.pending_foreground());
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["goal"]["objective"],
        "Existing"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn an_extension_owned_initial_goal_is_delivered_to_the_extension() {
    let (root, client, spec) = fixture_with(&[("OMP_FIXTURE_GOAL", "1")]);
    std::fs::create_dir_all(root.path().join("agent")).unwrap();
    std::fs::write(
        root.path().join("agent/plugins.json"),
        json!([
            {"name":"goal", "source":"extension", "description":"Custom goal command"}
        ])
        .to_string(),
    )
    .unwrap();
    let (execution, _) = provider::testing::worker_at(
        root.path(),
        std::path::Path::new(&spec.cwd),
        model::creation::Creations::default(),
        Box::new(client),
    );
    let created = execution
        .execute(
            "agent.create.request",
            json!({
                "config":{"provider":"omp","cwd":spec.cwd},"initialPrompt":"/goal Custom objective"
            }),
        )
        .await
        .unwrap();
    assert_eq!(created["status"], "agent_created");
    execution.shutdown().await.unwrap();
    let calls = calls_after(root.path(), 0);
    assert_eq!(count(&calls, "goal"), 0);
    assert!(
        calls
            .iter()
            .any(|call| call["type"] == "prompt" && call["message"] == "/goal Custom objective")
    );
}
