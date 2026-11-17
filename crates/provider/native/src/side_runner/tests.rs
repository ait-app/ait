use super::*;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::AgentSessionSpec;
use provider_core::ports::environment::AgentEnvironment;
use provider_core::protocol::side_questions::{SideContextEntry, SideContextSnapshot};
use serde_json::Value;

fn spec() -> SideRunnerSpec {
    SideRunnerSpec {
        session: AgentSessionSpec {
            provider: "fixture".into(),
            cwd: "/tmp".into(),
            config: StoredAgentConfig {
                model: Some("selected-model".into()),
                ..StoredAgentConfig::default()
            },
            confinement: None,
        },
        directory: "/tmp/aux".into(),
        environment: AgentEnvironment::default(),
    }
}

fn ask(request_id: &str) -> SideAsk {
    SideAsk {
        request_id: request_id.into(),
        question: "why?".into(),
        record_id: None,
        previous: None,
        snapshot: SideContextSnapshot {
            agent_id: "agent".into(),
            v: 1,
            provider: "fixture".into(),
            model: Some("selected-model".into()),
            captured_at: 1,
            native_session_id: None,
            native_leaf_id: Some("leaf".into()),
            entries: vec![SideContextEntry {
                role: "user".into(),
                text: "frozen".into(),
            }],
            truncated: true,
        },
    }
}

fn completed(strategy: SideStrategy) -> SideTopic {
    let mut topic = topic(&ask("first"), &spec(), strategy).unwrap();
    topic.record.status = SideTurnStatus::Complete;
    topic.record.answer = "answer".into();
    topic
}

fn follow_up(previous: SideTopic) -> SideAsk {
    let mut follow = ask("second");
    follow.record_id = Some(previous.record.id.clone());
    follow.previous = Some(previous);
    follow
}

#[test]
fn new_topic_runs_with_the_requested_strategy_and_snapshot_anchor() {
    let topic = topic(&ask("first"), &spec(), SideStrategy::Native).unwrap();

    assert_eq!(topic.strategy, SideStrategy::Native);
    assert_eq!(topic.record.question, "why?");
    assert_eq!(topic.record.status, SideTurnStatus::Running);
    assert_eq!(topic.record.leaf_id.as_deref(), Some("leaf"));
    assert!(topic.truncated);
    assert!(topic.record.follow_ups.is_empty());
}

#[test]
fn follow_up_appends_a_running_turn_to_a_completed_topic_of_the_same_strategy() {
    let previous = completed(SideStrategy::Isolated);

    let topic = topic(
        &follow_up(previous.clone()),
        &spec(),
        SideStrategy::Isolated,
    )
    .unwrap();

    assert_eq!(topic.record.id, previous.record.id);
    assert_eq!(topic.record.answer, "answer");
    assert_eq!(topic.record.follow_ups.len(), 1);
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Running);
}

#[test]
fn invalid_asks_and_inconsistent_follow_ups_are_rejected() {
    let spec = spec();
    let mut cases = vec![ask("")];
    let mut blank = ask("blank");
    blank.question = "  ".into();
    cases.push(blank);
    let mut long = ask("long");
    long.question = "x".repeat(MAX_QUESTION_BYTES + 1);
    cases.push(long);
    let mut other_model = ask("model");
    other_model.snapshot.model = Some("other".into());
    cases.push(other_model);
    let mut other_provider = ask("provider");
    other_provider.snapshot.provider = "other".into();
    cases.push(other_provider);
    let mut other_version = ask("version");
    other_version.snapshot.v = 2;
    cases.push(other_version);
    let mut unexpected_previous = ask("previous");
    unexpected_previous.previous = Some(completed(SideStrategy::Isolated));
    cases.push(unexpected_previous);
    let mut missing_previous = ask("missing");
    missing_previous.record_id = Some("topic".into());
    cases.push(missing_previous);
    let mut wrong_record = follow_up(completed(SideStrategy::Isolated));
    wrong_record.record_id = Some("other-topic".into());
    cases.push(wrong_record);
    cases.push(follow_up(completed(SideStrategy::Native)));
    let mut unsettled = completed(SideStrategy::Isolated);
    unsettled.record.status = SideTurnStatus::Running;
    cases.push(follow_up(unsettled));

    for case in cases {
        assert!(
            matches!(
                topic(&case, &spec, SideStrategy::Isolated),
                Err(AgentSessionError::Rejected)
            ),
            "{}",
            case.request_id
        );
    }
}

#[test]
fn envelope_carries_snapshot_previous_record_and_question() {
    let previous = completed(SideStrategy::Isolated);
    let follow = follow_up(previous.clone());

    let envelope: Value = serde_json::from_str(&envelope(&follow).unwrap()).unwrap();

    assert_eq!(
        envelope["snapshot"],
        serde_json::to_value(&follow.snapshot).unwrap()
    );
    assert_eq!(
        envelope["previous"],
        serde_json::to_value(&previous.record).unwrap()
    );
    assert_eq!(envelope["question"], "why?");
}

#[test]
fn latest_turn_mut_targets_the_last_follow_up_or_the_root() {
    let mut root = completed(SideStrategy::Isolated);
    "root edit".clone_into(latest_turn_mut(&mut root).answer);
    assert_eq!(root.record.answer, "root edit");

    let mut followed = topic(&follow_up(root), &spec(), SideStrategy::Isolated).unwrap();
    let turn = latest_turn_mut(&mut followed);
    "follow-up edit".clone_into(turn.answer);
    *turn.truncated = false;
    assert_eq!(followed.record.answer, "root edit");
    assert_eq!(followed.record.follow_ups[0].answer, "follow-up edit");
    assert!(!followed.truncated);
}

#[test]
fn begin_refuses_running_and_repeated_requests_after_clearing_the_panel_error() {
    let mut panel = Panel::default();
    assert!(matches!(
        panel.admit(&"x".repeat(MAX_PROMPT_BYTES + 1)),
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(panel.rejection_reason(), Some(reason::CONTEXT_LIMIT));
    assert_eq!(
        panel.projection(false).error.as_deref(),
        Some(CONTEXT_LIMIT_MESSAGE)
    );
    assert!(panel.take_changed());

    assert!(matches!(
        panel.begin("first", true),
        Err(AgentSessionError::Busy)
    ));
    assert_eq!(panel.rejection_reason(), None);
    assert!(panel.take_changed());
    assert!(panel.admit(&"x".repeat(MAX_PROMPT_BYTES)).is_ok());

    panel.install(
        topic(&ask("first"), &spec(), SideStrategy::Isolated).unwrap(),
        "first".into(),
    );
    assert!(matches!(
        panel.begin("first", false),
        Err(AgentSessionError::Failed)
    ));
    assert!(panel.begin("second", false).is_ok());
    assert!(!panel.take_changed());
}

#[test]
fn projection_ownership_and_history_follow_the_installed_topic() {
    let mut panel = Panel::default();
    assert!(panel.owns(None));
    assert!(!panel.owns(Some("topic")));
    assert!(panel.history().is_empty());
    assert_eq!(panel.latest_status(), None);

    let topic = topic(&ask("first"), &spec(), SideStrategy::Isolated).unwrap();
    let id = topic.record.id.clone();
    panel.install(topic, "first".into());

    assert!(panel.owns(Some(&id)));
    assert!(!panel.owns(Some("other")));
    assert_eq!(panel.request_id(), Some("first"));
    assert_eq!(panel.history().len(), 1);
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Running));
    let running = panel.projection(true);
    assert_eq!(running.running_id.as_deref(), Some(id.as_str()));
    assert_eq!(running.reveal.as_deref(), Some(id.as_str()));
    assert_eq!(running.records.len(), 1);
    assert_eq!(panel.projection(false).running_id, None);
}

#[test]
fn finish_keeps_a_native_error_reports_the_fallback_and_clears_on_success() {
    let mut panel = Panel::default();
    panel.finish(SideTurnStatus::Error, "fallback");
    assert!(!panel.take_changed());
    panel.install(
        topic(&ask("first"), &spec(), SideStrategy::Isolated).unwrap(),
        "first".into(),
    );

    panel.native_error("native failure");
    panel.finish(SideTurnStatus::Error, "fallback");
    assert_eq!(
        panel.topic().unwrap().latest_turn().error,
        Some("native failure")
    );
    assert!(panel.take_changed());

    panel.topic_mut().unwrap().record.error = None;
    panel.finish(SideTurnStatus::Error, "fallback");
    assert_eq!(panel.topic().unwrap().latest_turn().error, Some("fallback"));

    panel.finish(SideTurnStatus::Complete, "fallback");
    let turn = panel.topic().unwrap().latest_turn();
    assert_eq!(turn.status, SideTurnStatus::Complete);
    assert_eq!(turn.error, None);
    assert!(turn.updated_at > 0);
}

#[tokio::test]
async fn cleanup_settles_the_status_its_task_returns() {
    let mut panel = Panel::default();
    panel.install(
        topic(&ask("first"), &spec(), SideStrategy::Isolated).unwrap(),
        "first".into(),
    );
    let mut cleanup = Cleanup::default();
    cleanup.settle(&mut panel, "fallback").await.unwrap();
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Running));

    cleanup.start(async { Ok(SideTurnStatus::Cancelled) });
    assert!(cleanup.is_pending());
    cleanup.settle(&mut panel, "fallback").await.unwrap();
    assert!(!cleanup.is_pending());
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Cancelled));

    cleanup.start(async { Err(AgentSessionError::Failed) });
    assert!(matches!(
        cleanup.settle(&mut panel, "fallback").await,
        Err(AgentSessionError::Failed)
    ));
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Error));
}

#[tokio::test]
async fn reap_settles_only_finished_cleanup_and_treats_failure_as_error() {
    let mut panel = Panel::default();
    panel.install(
        topic(&ask("first"), &spec(), SideStrategy::Isolated).unwrap(),
        "first".into(),
    );
    let mut cleanup = Cleanup::default();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    cleanup.start(async move {
        let _ = released.await;
        Err(AgentSessionError::Failed)
    });
    cleanup.reap(&mut panel, "fallback");
    assert!(cleanup.is_pending());
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Running));

    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while cleanup.is_pending() {
            cleanup.reap(&mut panel, "fallback");
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(panel.latest_status(), Some(SideTurnStatus::Error));
    assert_eq!(panel.topic().unwrap().latest_turn().error, Some("fallback"));
}
