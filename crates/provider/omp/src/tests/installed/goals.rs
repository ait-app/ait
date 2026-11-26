use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; exercises native Goal RPC with isolated loopback configuration"]
async fn installed_omp_goal_rpc_and_restoration_do_not_require_a_slash_completion() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    // OMP writes the native session file after its first assistant response.
    session
        .start_turn("Initialize isolated session", &fixture.spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    while session.command_catalog().is_none() {
        next(session.as_mut()).await.unwrap();
    }
    assert!(
        session
            .command_catalog()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "goal")
    );
    assert_eq!(
        session.runtime_info().await.unwrap().extra.unwrap()["goal"],
        Value::Null
    );
    for (command, status) in [
        ("/goal Verify isolated native Goal RPC", "active"),
        ("/goal pause", "paused"),
    ] {
        session
            .out_of_band(&AgentPrompt::text(command))
            .await
            .unwrap();
        wait_goal(session.as_mut(), |goal| goal["status"] == status).await;
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let mut restored = fixture
        .client
        .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let goal = restored.runtime_info().await.unwrap().extra.unwrap()["goal"].clone();
    assert_eq!(goal["objective"], "Verify isolated native Goal RPC");
    assert_eq!(goal["status"], "paused");
    while restored.command_catalog().is_none() {
        next(restored.as_mut()).await.unwrap();
    }
    restored
        .out_of_band(&AgentPrompt::text("/goal clear"))
        .await
        .unwrap();
    wait_goal(restored.as_mut(), Value::is_null).await;
    restored.close().await.unwrap();
}

async fn wait_goal(session: &mut dyn AgentSession, expected: impl Fn(&Value) -> bool) {
    loop {
        let event = next(session).await.unwrap();
        if let AgentTurnEvent::RuntimeInfo(info) = event
            && let Some(goal) = info.extra.as_ref().and_then(|extra| extra.get("goal"))
            && expected(goal)
        {
            return;
        }
    }
}
