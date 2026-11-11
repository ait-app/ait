use std::time::Duration;

use domain::agent_runtime::goal::{GoalControlOp, GoalState};

use crate::ports::agent_session::{AgentClient, AgentSession, AgentSessionError, AgentTurnEvent};
use crate::ports::goal::GoalPauseSemantics;
use crate::test_support::Fixture;

async fn lifecycle_event(session: &mut dyn AgentSession) -> AgentTurnEvent {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap()
                && matches!(
                    event,
                    AgentTurnEvent::Started(_)
                        | AgentTurnEvent::Completed(_)
                        | AgentTurnEvent::Cancelled
                )
            {
                return event;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn eligibility_is_bound_to_the_admitted_session_not_another_clients_probe() {
    let proven = Fixture::new();
    proven.mode("goal-native");
    let unproven = Fixture::new();
    unproven.mode("goal-native");
    std::fs::write(unproven.cwd.join("goal-version"), "0.166.0").unwrap();
    let client = proven.client();
    let mut native = client.create_session(&proven.spec()).await.unwrap();
    let mut fallback = client.create_session(&unproven.spec()).await.unwrap();
    assert!(native.goal_capability().native);
    assert_eq!(
        native.goal_capability().pause,
        GoalPauseSemantics::DeferredFinish
    );
    assert!(!fallback.goal_capability().native);
    assert!(fallback.goal_capability().reason.is_some());
    assert_eq!(
        fallback.goal_create("must not write").await,
        Err(AgentSessionError::Unavailable)
    );
    assert!(
        !unproven
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/goal/set")
    );
    native.close().await.unwrap();
    fallback.close().await.unwrap();
}

#[tokio::test]
async fn pause_finishes_the_native_turn_and_resume_starts_a_new_native_turn_without_prompts() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    let created = session.goal_create("ship the change").await.unwrap();
    assert_eq!(created.state, GoalState::Active);
    let AgentTurnEvent::Started(first) = lifecycle_event(session.as_mut()).await else {
        panic!("native turn must start")
    };
    let paused = session.goal_control(GoalControlOp::Pause).await.unwrap();
    assert_eq!(paused.state, GoalState::Paused);
    assert_eq!(paused.objective, created.objective);
    assert!(matches!(
        lifecycle_event(session.as_mut()).await,
        AgentTurnEvent::Completed(_)
    ));
    let read = session.goal_read().await.unwrap().unwrap();
    assert_eq!(read.state, GoalState::Paused);
    assert!(!session.pending_foreground());
    session.goal_control(GoalControlOp::Resume).await.unwrap();
    let AgentTurnEvent::Started(second) = lifecycle_event(session.as_mut()).await else {
        panic!("resume must start a native turn")
    };
    assert_ne!(first, second);
    session.goal_control(GoalControlOp::Pause).await.unwrap();
    assert!(matches!(
        lifecycle_event(session.as_mut()).await,
        AgentTurnEvent::Completed(_)
    ));
    assert!(!fixture.requests().iter().any(|request| matches!(
        request["method"].as_str(),
        Some("turn/start" | "turn/interrupt")
    )));
    assert!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "thread/goal/set")
            .all(|request| request["params"]["origin"] == "user")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn stop_interrupts_before_pausing_and_clear_is_not_completion() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    session
        .goal_create("preserve this objective")
        .await
        .unwrap();
    assert!(matches!(
        lifecycle_event(session.as_mut()).await,
        AgentTurnEvent::Started(_)
    ));
    let stopped = session.goal_control(GoalControlOp::Stop).await.unwrap();
    assert_eq!(stopped.state, GoalState::Paused);
    assert_eq!(stopped.objective, "preserve this objective");
    assert_eq!(
        lifecycle_event(session.as_mut()).await,
        AgentTurnEvent::Cancelled
    );
    assert_eq!(
        session.goal_read().await.unwrap().unwrap().state,
        GoalState::Paused
    );
    let requests = fixture.requests();
    let interrupt = requests
        .iter()
        .position(|request| request["method"] == "turn/interrupt")
        .unwrap();
    let pause = requests
        .iter()
        .position(|request| {
            request["method"] == "thread/goal/set" && request["params"]["status"] == "paused"
        })
        .unwrap();
    assert!(interrupt < pause);
    let cleared = session.goal_control(GoalControlOp::Clear).await.unwrap();
    assert_eq!(cleared.state, GoalState::Cleared);
    assert_eq!(cleared.objective, "preserve this objective");
    assert_eq!(session.goal_read().await.unwrap(), None);
    session.close().await.unwrap();
}

#[tokio::test]
async fn failed_pause_after_interrupt_is_uncertain_not_a_definitive_rejection() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    session.goal_create("continue until stopped").await.unwrap();
    assert!(matches!(
        lifecycle_event(session.as_mut()).await,
        AgentTurnEvent::Started(_)
    ));
    std::fs::write(fixture.cwd.join("goal-reject-pause"), "").unwrap();
    assert_eq!(
        session.goal_control(GoalControlOp::Stop).await,
        Err(AgentSessionError::Failed)
    );
    assert_eq!(
        session.goal_read().await.unwrap().unwrap().state,
        GoalState::Active
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn uncertain_native_create_is_not_retried_by_the_adapter() {
    let fixture = Fixture::new();
    fixture.mode("goal-uncertain-set");
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    assert_eq!(
        session.goal_create("write then disconnect").await,
        Err(AgentSessionError::Failed)
    );
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.cwd.join("goal-state.json")).unwrap())
            .unwrap();
    assert_eq!(state["objective"], "write then disconnect");
    let writes = fixture
        .requests()
        .into_iter()
        .filter(|request| request["method"] == "thread/goal/set")
        .count();
    assert_eq!(
        writes, 1,
        "an uncertain native mutation must never be resent"
    );
    let _ = session.close().await;
}

#[tokio::test]
async fn arbiter_reassert_has_automatic_origin_and_no_prompt() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    std::fs::write(
        fixture.cwd.join("goal-state.json"),
        serde_json::json!({
            "threadId":"native", "objective":"reconciled run", "status":"active",
            "createdAt":1,"updatedAt":2,"tokensUsed":13,"timeUsedSeconds":1,"tokenBudget":null
        })
        .to_string(),
    )
    .unwrap();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    let facts = session.goal_reassert().await.unwrap();
    assert_eq!(facts.state, GoalState::Active);
    assert_eq!(facts.objective, "reconciled run");
    let requests = fixture.requests();
    let reassert = requests
        .iter()
        .find(|request| request["method"] == "thread/goal/set")
        .unwrap();
    assert_eq!(reassert["params"]["origin"], "automatic");
    assert!(reassert["params"].get("objective").is_none());
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn clear_that_removes_nothing_is_rejected_not_reported_as_cleared() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    session.goal_create("vanishing objective").await.unwrap();
    std::fs::write(fixture.cwd.join("goal-clear-noop"), "").unwrap();
    assert_eq!(
        session.goal_control(GoalControlOp::Clear).await,
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.goal_read().await.unwrap().unwrap().state,
        GoalState::Active
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn prerelease_runtime_does_not_inherit_the_release_pause_proof() {
    let fixture = Fixture::new();
    fixture.mode("goal-native");
    std::fs::write(fixture.cwd.join("goal-version"), "0.167.0-alpha.1").unwrap();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    assert!(!session.goal_capability().native);
    assert_eq!(
        session.goal_control(GoalControlOp::Pause).await,
        Err(AgentSessionError::Unavailable)
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "thread/goal/set")
    );
    session.close().await.unwrap();
}
