use super::*;

#[test]
fn foreground_admission_separates_objectives_from_goal_controls() {
    for input in [
        "/goal fix the tests",
        "/goal set release criteria",
        "/guided-goal",
        "/guided-goal rough objective",
    ] {
        assert!(starts_goal(input), "{input}");
    }
    for input in [
        "/goal",
        "/goal show",
        "/goal pause",
        "/goal resume",
        "/goal drop",
        "/goal budget 200",
        "/other goal",
    ] {
        assert!(!starts_goal(input), "{input}");
    }
}

async fn bridged() -> (
    tempfile::TempDir,
    Session,
    crate::ports::agent_session::AgentSessionSpec,
) {
    use crate::ports::agent_session::AgentSession;
    let (root, client, spec) = crate::local::omp::tests::fixture();
    let mut session = crate::local::omp::session::open(&client, &spec, None)
        .await
        .unwrap();
    let handle = session.persistence().unwrap();
    session.vibe_host = Some(Host::new(
        &client,
        &spec,
        &handle.session_id,
        session.state["sessionFile"].as_str().unwrap(),
        None,
    ));
    (root, session, spec)
}

#[tokio::test]
async fn goal_objectives_use_native_state_and_never_silently_replace_an_existing_goal() {
    use crate::ports::agent_session::AgentSession;
    let (root, mut session, spec) = bridged().await;
    session
        .start_input(&AgentPrompt::text("/goal verify the release"), &spec.config)
        .await
        .unwrap();
    assert_eq!(
        session.state["goal"]["goal"]["objective"],
        "verify the release"
    );
    assert!(session.stream.events.iter().any(|event| matches!(event, AgentTurnEvent::Timeline(note) if note.item["goal"]["phase"] == "active")));
    crate::local::omp::tests::complete(&mut session).await;
    let calls: Vec<Value> = std::fs::read_to_string(root.path().join("agent/calls.jsonl"))
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let prepared = calls
        .iter()
        .position(|call| call["message"] == "/ait-goal-tools-v1")
        .unwrap();
    let created = calls
        .iter()
        .position(|call| call["type"] == "goal" && call["op"] == "create")
        .unwrap();
    assert!(prepared < created);
    assert!(
        session
            .start_input(&AgentPrompt::text("/goal another objective"), &spec.config)
            .await
            .is_err()
    );
    assert_eq!(
        session.state["goal"]["goal"]["objective"],
        "verify the release"
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn goal_controls_do_not_send_prompts_and_guided_interviews_do_not_enable_a_goal() {
    use crate::ports::agent_session::AgentSession;
    let (root, mut session, _spec) = bridged().await;
    let prepared = session
        .prepare_goal(&AgentPrompt::text("/guided-goal rough objective"))
        .await
        .unwrap();
    assert_eq!(
        prepared.prompt.unwrap().text,
        "/ait-guided-goal-v1 rough objective"
    );
    assert!(session.state["goal"].is_null());
    session
        .goal_control(&AgentPrompt::text("/goal show"))
        .await
        .unwrap();
    session
        .goal_control(&AgentPrompt::text("/goal drop"))
        .await
        .unwrap();
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(
        !calls
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .any(|call| call["type"] == "prompt")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn current_goal_controls_reject_stale_identity_before_modifying_native_state() {
    use crate::ports::agent_session::AgentSession;
    let (root, mut session, spec) = bridged().await;
    session
        .start_input(
            &AgentPrompt::text("/goal Persist the controls"),
            &spec.config,
        )
        .await
        .unwrap();
    crate::local::omp::tests::complete(&mut session).await;
    let projection = session.ui_state().unwrap();
    let goal = &projection["goal"]["goal"];
    assert_eq!(goal["phase"], "active");
    assert_eq!(goal["objective"], "Persist the controls");
    assert!(!projection["statuses"].to_string().contains("Goal on"));
    let active_goal = session.state["goal"].clone();
    session.state["goal"]["goal"]["status"] = json!("complete");
    assert!(session.ui_state().unwrap()["goal"].is_null());
    session.state["goal"] = active_goal;
    let before = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(
        session
            .begin_action("goal", &json!({"op":"drop", "expectedGoalId":"stale"}))
            .await
            .is_err()
    );
    let after = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(!after[before.len()..].contains("\"op\": \"drop\""));
    let id = goal["id"].as_str().unwrap();
    session
        .begin_action("goal", &json!({"op":"pause", "expectedGoalId":id}))
        .await
        .unwrap()
        .await
        .unwrap();
    assert_eq!(
        session.ui_state().unwrap()["goal"]["goal"]["phase"],
        "paused"
    );
    session
        .begin_action("goal", &json!({"op":"drop", "expectedGoalId":id}))
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(session.ui_state().unwrap()["goal"].is_null());
    session.close().await.unwrap();
}

#[tokio::test]
async fn acknowledged_goal_snapshot_supersedes_older_buffered_updates_but_keeps_turn_frames() {
    use crate::ports::agent_session::AgentSession;
    let (_root, mut session, spec) = bridged().await;
    session
        .start_input(
            &AgentPrompt::text("/goal Keep the current controls accurate"),
            &spec.config,
        )
        .await
        .unwrap();
    crate::local::omp::tests::complete(&mut session).await;
    let goal = session.state["goal"].clone();
    session
        .transport
        .restore_events(vec![
            json!({"type":"goal_updated", "state":goal}),
            json!({"type":"agent_end"}),
        ])
        .unwrap();
    session
        .goal_control(&AgentPrompt::text("/goal pause"))
        .await
        .unwrap();
    let retained = session.transport.poll().unwrap().unwrap();
    assert_eq!(retained["type"], "agent_end");
    assert_eq!(
        session.ui_state().unwrap()["goal"]["goal"]["phase"],
        "paused"
    );
    session.close().await.unwrap();
}

#[test]
fn completed_goal_is_a_historical_receipt_not_an_active_control() {
    for status in ["complete", "completed"] {
        let note = goal_item(
            "current",
            &json!({"goal":{"id":"done","status":status,"objective":"Verified"}}),
        );
        assert_eq!(note.item["goal"]["phase"], "complete");
        assert_eq!(note.item["actions"], json!([]));
    }
}
