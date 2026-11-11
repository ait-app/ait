//! Admission-read contract: authoritative current reads, exact native error taxonomy.

use domain::agent_runtime::goal::GoalState;
use serde_json::json;

use super::facts;
use crate::ports::agent_session::AgentClient;
use crate::test_support::Fixture;

#[test]
fn native_statuses_project_to_typed_states_without_host_guesses() {
    for (status, state) in [
        ("active", GoalState::Active),
        ("paused", GoalState::Paused),
        ("blocked", GoalState::Blocked),
        ("complete", GoalState::Completed),
        ("usageLimited", GoalState::Blocked),
        ("budgetLimited", GoalState::Blocked),
    ] {
        let goal = json!({"threadId":"thread","objective":"ship it","status":status,
            "createdAt":1_794_383_673,"updatedAt":1_794_383_679,
            "tokensUsed":26,"timeUsedSeconds":12,"tokenBudget":13});
        let facts = facts(&goal).unwrap();
        assert_eq!(facts.state, state, "{status}");
        assert_eq!(facts.objective, "ship it");
        assert_eq!(facts.native_status, status);
        assert_eq!(facts.revision.as_deref(), Some("1794383679"));
        assert_eq!(facts.native_id, None);
        let meters = facts.meters.unwrap();
        assert_eq!(meters["tokensUsed"], 26);
        assert_eq!(meters["tokenBudget"], 13);
    }
}

#[test]
fn unknown_status_or_missing_fields_fail_instead_of_guessing() {
    let base = json!({"threadId":"thread","objective":"ship it","status":"active",
        "createdAt":1,"updatedAt":2,"tokensUsed":0,"timeUsedSeconds":0});
    let mut unknown = base.clone();
    unknown["status"] = json!("futureStatus");
    assert!(facts(&unknown).is_err());
    let mut no_objective = base.clone();
    no_objective["objective"] = serde_json::Value::Null;
    assert!(facts(&no_objective).is_err());
    let mut no_status = base;
    no_status["status"] = serde_json::Value::Null;
    assert!(facts(&no_status).is_err());
}

#[tokio::test]
async fn goal_read_returns_absence_only_from_an_authoritative_null() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    assert_eq!(session.goal_read().await.unwrap(), None);
    let reads: Vec<_> = fixture
        .requests()
        .into_iter()
        .filter(|request| request["method"] == "thread/goal/get")
        .collect();
    assert_eq!(reads.len(), 1);
    session.close().await.unwrap();
}

#[tokio::test]
async fn goal_read_projects_the_current_native_goal() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.cwd.join("goal-state.json"),
        json!({"threadId":"thread","objective":"ship it","status":"paused",
            "createdAt":1_794_383_673,"updatedAt":1_794_383_677,
            "tokensUsed":13,"timeUsedSeconds":5,"tokenBudget":serde_json::Value::Null})
        .to_string(),
    )
    .unwrap();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    let facts = session.goal_read().await.unwrap().unwrap();
    assert_eq!(facts.state, GoalState::Paused);
    assert_eq!(facts.objective, "ship it");
    assert_eq!(facts.revision.as_deref(), Some("1794383677"));
    assert_eq!(
        facts.meters.unwrap()["tokenBudget"],
        serde_json::Value::Null
    );
    session.close().await.unwrap();
}

#[tokio::test]
async fn feature_disabled_is_unavailable_not_absence() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.cwd.join("goal-get-error.json"),
        json!({"code":-32600,"message":"goals feature is disabled"}).to_string(),
    )
    .unwrap();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    assert!(matches!(
        session.goal_read().await,
        Err(crate::ports::agent_session::AgentSessionError::Unavailable)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn other_native_goal_errors_stay_definitive_rejections() {
    for message in [
        "cannot update goal for thread abc: no goal exists",
        "ephemeral thread does not support goals",
        "invalid thread id: abc",
    ] {
        let fixture = Fixture::new();
        std::fs::write(
            fixture.cwd.join("goal-get-error.json"),
            json!({"code":-32600,"message":message}).to_string(),
        )
        .unwrap();
        let client = fixture.client();
        let mut session = client.create_session(&fixture.spec()).await.unwrap();
        assert!(
            matches!(
                session.goal_read().await,
                Err(crate::ports::agent_session::AgentSessionError::Rejected)
            ),
            "{message}"
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_goal_field_does_not_prove_absence() {
    let fixture = Fixture::new();
    std::fs::write(fixture.cwd.join("goal-get-response.json"), "{}").unwrap();
    let client = fixture.client();
    let mut session = client.create_session(&fixture.spec()).await.unwrap();
    assert!(matches!(
        session.goal_read().await,
        Err(crate::ports::agent_session::AgentSessionError::Failed)
    ));
    session.close().await.unwrap();
}
