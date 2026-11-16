//! Pi BTW runner behavior against the deterministic native peer.

use std::time::Duration;

use tempfile::TempDir;

use super::*;
use crate::tests::fixture;
use domain::agent_runtime::StoredAgentConfig;
use provider_core::ports::agent_session::AgentSessionSpec;
use provider_core::ports::environment::AgentEnvironment;
use provider_core::protocol::side_questions::SideContextEntry;

fn spec(directory: &TempDir, session: AgentSessionSpec) -> SideRunnerSpec {
    SideRunnerSpec {
        session: AgentSessionSpec {
            config: StoredAgentConfig {
                model: Some("fixture/model".into()),
                ..session.config.clone()
            },
            ..session
        },
        directory: directory.path().join("btw"),
        environment: AgentEnvironment::default(),
    }
}

async fn runner(scenario: &str) -> (TempDir, Runner, TempDir) {
    let (native, client, session) = fixture(scenario);
    let owned = tempfile::tempdir().unwrap();
    let spec = spec(&owned, session);
    let mut runner = Runner::new(client, &spec).unwrap();
    runner.probe().await.unwrap();
    (native, runner, owned)
}

fn snapshot() -> SideContextSnapshot {
    SideContextSnapshot {
        v: 1,
        agent_id: "agent".into(),
        provider: "pi".into(),
        model: Some("fixture/model".into()),
        native_session_id: Some("native".into()),
        native_leaf_id: None,
        captured_at: 1,
        entries: vec![
            SideContextEntry {
                role: "user".into(),
                text: "MAIN-ALPHA explain the parser".into(),
            },
            SideContextEntry {
                role: "assistant".into(),
                text: "MAIN-BETA the tokenizer splits tokens".into(),
            },
        ],
        truncated: false,
    }
}

fn ask(question: &str) -> SideAsk {
    SideAsk {
        request_id: format!("request-{question}"),
        question: question.into(),
        record_id: None,
        snapshot: snapshot(),
        previous: None,
    }
}

/// Wait until the single retained topic reaches a terminal status.
async fn settled(runner: &mut Runner) -> SideTopic {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let topics = runner.side_history().await.unwrap();
            if let Some(topic) = topics.first()
                && topic.latest_turn().status != SideTurnStatus::Running
            {
                return topic.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("side turn did not settle")
}

fn prompts(native: &TempDir) -> Vec<String> {
    let path = native.path().join("requests.jsonl");
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|command| command["type"] == "prompt")
        .filter_map(|command| command["message"].as_str().map(str::to_owned))
        .collect()
}

fn arguments(native: &TempDir) -> Vec<String> {
    serde_json::from_str(&std::fs::read_to_string(native.path().join("arguments.json")).unwrap())
        .unwrap()
}

#[test]
fn closed_channel_admits_only_the_documented_feature_set() {
    for kind in [
        "get_state",
        "set_model",
        "set_thinking_level",
        "prompt",
        "abort",
    ] {
        assert!(allowed(kind), "{kind}");
    }
    for kind in [
        "bash",
        "abort_bash",
        "steer",
        "follow_up",
        "compact",
        "fork",
        "clone",
        "switch_session",
        "new_session",
        "clear_queue",
        "export_html",
        "get_commands",
        "extension_ui_response",
    ] {
        assert!(!allowed(kind), "{kind}");
    }
}

#[tokio::test]
async fn launch_uses_only_public_denial_flags_and_writes_no_native_state() {
    let (native, mut runner, _owned) = runner("").await;
    let receipt = runner
        .side_ask(ask("what did the snapshot say?"))
        .await
        .unwrap();
    assert_eq!(receipt.strategy, SideStrategy::Isolated);
    let topic = settled(&mut runner).await;
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Complete);
    let arguments = arguments(&native);
    assert_eq!(
        arguments
            .iter()
            .filter(|argument| argument.starts_with("--no-"))
            .cloned()
            .collect::<Vec<_>>(),
        ISOLATION.to_vec()
    );
    for forbidden in [
        "--session-dir",
        "--session-id",
        "--session",
        "--extension",
        "--approve",
    ] {
        assert!(
            !arguments.iter().any(|argument| argument == forbidden),
            "{forbidden}"
        );
    }
    // A stateless process must leave no native session or auxiliary artifact.
    assert!(!native.path().join("session.jsonl").exists());
    assert!(!native.path().join("btw").exists());
    // The prompt carries the frozen snapshot verbatim and nothing else from main.
    let prompts = prompts(&native);
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("MAIN-ALPHA explain the parser"));
    assert!(prompts[0].contains("MAIN-BETA the tokenizer splits tokens"));
    assert!(prompts[0].ends_with("[BTW question]\nwhat did the snapshot say?"));
    assert_eq!(
        topic.latest_turn().answer,
        format!("SIDE-ANSWER::{}", prompts[0].trim_end())
    );
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn follow_up_replays_frozen_snapshot_and_stored_turns_only() {
    let (native, mut runner, _owned) = runner("").await;
    runner.side_ask(ask("first question")).await.unwrap();
    let first = settled(&mut runner).await;
    let follow = SideAsk {
        request_id: "request-follow".into(),
        question: "second question".into(),
        record_id: Some(first.record.id.clone()),
        snapshot: snapshot(),
        previous: Some(first.clone()),
    };
    let receipt = runner.side_ask(follow).await.unwrap();
    assert_eq!(receipt.topic.record.turn_count(), 2);
    let second = settled(&mut runner).await;
    assert_eq!(second.record.turn_count(), 2);
    assert_eq!(second.latest_turn().status, SideTurnStatus::Complete);
    let prompts = prompts(&native);
    assert_eq!(prompts.len(), 2);
    let replay = &prompts[1];
    assert!(replay.contains("MAIN-ALPHA explain the parser"));
    assert!(replay.contains("[BTW prior question]\nfirst question"));
    assert!(replay.contains(first.latest_turn().answer));
    assert!(replay.ends_with("[BTW question]\nsecond question"));
    // Both turns are retained for the daemon store projection.
    let history = runner.side_history().await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].record.question, "first question");
    assert_eq!(history[0].record.follow_ups.len(), 1);
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn cancel_confirms_native_settlement_and_never_reports_unconfirmed_success() {
    let (_native, mut runner, _owned) = runner("").await;
    runner.side_ask(ask("SIDE-BLOCK")).await.unwrap();
    let cancelled = runner.side_cancel(None).await.unwrap();
    assert!(cancelled.cancelled);
    let topic = settled(&mut runner).await;
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Cancelled);
    // Nothing is running afterwards, so a second cancel is a no-op.
    assert!(!runner.side_cancel(None).await.unwrap().cancelled);
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn process_death_mid_turn_settles_interrupted_without_resubmission() {
    let (native, mut runner, _owned) = runner("").await;
    runner.side_ask(ask("SIDE-DIE-AFTER")).await.unwrap();
    let topic = settled(&mut runner).await;
    assert_eq!(topic.latest_turn().status, SideTurnStatus::Interrupted);
    assert_eq!(prompts(&native).len(), 1);
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn second_ask_while_running_is_busy_and_handled_prompt_is_rejected() {
    let (_native, mut runner, _owned) = runner("").await;
    runner.side_ask(ask("SIDE-BLOCK")).await.unwrap();
    assert!(matches!(
        runner.side_ask(ask("another")).await,
        Err(AgentSessionError::Busy)
    ));
    runner.side_cancel(None).await.unwrap();
    settled(&mut runner).await;
    assert!(matches!(
        runner.side_ask(ask("SIDE-HANDLED")).await,
        Err(AgentSessionError::Rejected)
    ));
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn reconcile_returns_the_admitted_topic_and_none_for_unknown_requests() {
    let (_native, mut runner, _owned) = runner("").await;
    let mut first = ask("reconcat");
    first.request_id = "request-known".into();
    runner.side_ask(first).await.unwrap();
    let topic = settled(&mut runner).await;
    let reconciled = runner.reconcile_side("request-known").await.unwrap();
    assert_eq!(
        reconciled.map(|topic| topic.record.id),
        Some(topic.record.id)
    );
    // Nothing native survives, so an unknown request is never guessed.
    assert!(
        runner
            .reconcile_side("request-unknown")
            .await
            .unwrap()
            .is_none()
    );
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn missing_runtime_and_old_version_remove_only_the_side_capability() {
    let (native, client, session) = fixture("");
    let owned = tempfile::tempdir().unwrap();
    let missing = SideRunnerSpec {
        session: AgentSessionSpec {
            config: StoredAgentConfig {
                model: Some("fixture/model".into()),
                ..session.config.clone()
            },
            ..session.clone()
        },
        directory: owned.path().join("btw"),
        environment: AgentEnvironment::default(),
    };
    let gone = PiClient::new(native.path().join("absent"));
    let mut runner = Runner::new(gone, &missing).unwrap();
    assert!(matches!(
        runner.probe().await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(!runner.side_capability().available);
    assert!(matches!(
        runner.side_ask(ask("unreachable")).await,
        Err(AgentSessionError::Unavailable)
    ));
    drop(client);

    let (_old_native, old_client, old_session) = fixture("old");
    let old_spec = SideRunnerSpec {
        session: AgentSessionSpec {
            config: StoredAgentConfig {
                model: Some("fixture/model".into()),
                ..old_session.config.clone()
            },
            ..old_session
        },
        directory: owned.path().join("btw-old"),
        environment: AgentEnvironment::default(),
    };
    let mut runner = Runner::new(old_client, &old_spec).unwrap();
    assert!(matches!(
        runner.probe().await,
        Err(AgentSessionError::Unavailable)
    ));
    assert!(!runner.side_capability().available);
}

#[tokio::test]
async fn topics_are_projected_newest_first_with_the_running_topic_marked_once() {
    let (_native, mut runner, _owned) = runner("").await;
    runner.side_ask(ask("first")).await.unwrap();
    settled(&mut runner).await;
    runner.side_ask(ask("SIDE-BLOCK")).await.unwrap();
    let projection = runner.poll_side().expect("running topic changed");
    assert_eq!(projection.records.len(), 2);
    assert_eq!(
        projection.running_id.as_deref(),
        Some(projection.records[0].record.id.as_str())
    );
    assert_eq!(
        projection.records[0].latest_turn().status,
        SideTurnStatus::Running
    );
    assert!(projection.records[0].record.created_at >= projection.records[1].record.created_at);
    assert!(runner.poll_side().is_none());
    runner.side_cancel(None).await.unwrap();
    settled(&mut runner).await;
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn overlong_questions_and_foreign_follow_ups_are_rejected_before_native_use() {
    let (native, mut runner, _owned) = runner("").await;
    let mut too_long = ask("x");
    too_long.question = "x".repeat(MAX_QUESTION_BYTES + 1);
    assert!(matches!(
        runner.side_ask(too_long).await,
        Err(AgentSessionError::Rejected)
    ));
    let mut unknown = ask("follow");
    unknown.record_id = Some("missing".into());
    assert!(matches!(
        runner.side_ask(unknown).await,
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(prompts(&native), Vec::<String>::new());
    assert_eq!(
        runner.side_history().await.unwrap(),
        Vec::<SideTopic>::new()
    );
    runner.close_side().await.unwrap();
}

#[test]
fn thinking_frames_never_enter_the_visible_answer() {
    let mut state = TurnState::default();
    state
        .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"thinking_delta", "delta":"private reasoning"
        }}))
        .unwrap();
    state
        .consume(&json!({"type":"message_update", "assistantMessageEvent":{
            "type":"text_delta", "delta":"visible answer"
        }}))
        .unwrap();
    assert_eq!(state.text, "visible answer");
}

#[test]
fn long_context_entries_are_preserved_and_oversized_prompts_fail_before_execution() {
    let mut context = snapshot();
    context.entries[0].text = "p".repeat(MAX_QUESTION_BYTES + 512);
    let prompt = render(&context, None, "follow this context").unwrap();
    assert!(prompt.contains(&context.entries[0].text));
    context.entries[0].text = "p".repeat(MAX_PROMPT_BYTES);
    assert!(matches!(
        render(&context, None, "question"),
        Err(ComposeError::ContextLimit)
    ));
}

#[test]
fn clipped_unicode_answers_are_marked_explicitly() {
    let mut state = TurnState::default();
    state.push(&"中".repeat(MAX_ANSWER_BYTES));
    assert!(state.truncated);
    assert!(state.text.len() <= MAX_ANSWER_BYTES);
    assert!(state.text.is_char_boundary(state.text.len()));
}

#[tokio::test]
async fn budget_rejection_exposes_a_recovery_action_without_sending_inference() {
    let (native, mut runner, _owned) = runner("").await;
    let mut question = ask("continue");
    question.snapshot.entries[0].text = "x".repeat(MAX_PROMPT_BYTES);
    assert!(matches!(
        runner.side_ask(question).await,
        Err(AgentSessionError::Rejected)
    ));
    assert_eq!(runner.side_rejection_reason(), Some("context_limit"));
    assert!(runner.side_projection().error.unwrap().contains("expand"));
    assert_eq!(prompts(&native), Vec::<String>::new());
}

#[tokio::test]
async fn replacement_runner_never_releases_a_live_execution_after_losing_ownership() {
    let (_native, mut runner, owned) = runner("").await;
    let mut request = ask("SIDE-BLOCK");
    request.request_id = "request-owned-recovery".into();
    let request_id = request.request_id.clone();
    runner.side_ask(request).await.unwrap();
    let mut replacement =
        Runner::new(runner.client.clone(), &spec(&owned, runner.session.clone())).unwrap();
    assert!(replacement.reconcile_side(&request_id).await.is_err());
    let marker = provider_native::auxiliary::path(owned.path().join("btw").as_path(), &request_id);
    assert!(
        !std::fs::read_to_string(&marker)
            .unwrap()
            .contains("SIDE-BLOCK")
    );
    runner.side_cancel(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if replacement.reconcile_side(&request_id).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(!marker.exists());
    runner.close_side().await.unwrap();
}

#[test]
fn continuous_follow_ups_preserve_turns_beyond_the_projection_window() {
    let (mut topic, _) = Runner::compose(&ask("first"), "first").unwrap();
    topic.record.status = SideTurnStatus::Complete;
    for index in 1..12 {
        topic = follow_up(topic, &format!("follow {index}"), index);
        topic.record.follow_ups.last_mut().unwrap().status = SideTurnStatus::Complete;
    }
    assert_eq!(topic.record.turn_count(), 12);
    assert_eq!(topic.record.follow_ups[0].question, "follow 1");
    assert_eq!(topic.latest_turn().question, "follow 11");
}

#[tokio::test]
async fn lost_acceptance_keeps_identity_and_never_resubmits_the_same_request() {
    let (native, mut runner, _owned) = runner("").await;
    runner.client.admission_deadline = Duration::from_millis(100);
    let question = ask("SIDE-NEVER-ADMIT");
    let request_id = question.request_id.clone();
    assert!(matches!(
        runner.side_ask(question.clone()).await,
        Err(AgentSessionError::Failed)
    ));
    let recovered = runner.reconcile_side(&request_id).await.unwrap().unwrap();
    assert_eq!(recovered.latest_turn().status, SideTurnStatus::Interrupted);
    let replayed = runner.side_ask(question).await.unwrap();
    assert_eq!(replayed.topic.record.id, recovered.record.id);
    assert_eq!(prompts(&native).len(), 1);
    runner.close_side().await.unwrap();
}

#[tokio::test]
async fn cancelled_admission_future_keeps_the_native_task_reachable_until_settlement() {
    let (native, mut runner, _owned) = runner("").await;
    runner.client.admission_deadline = Duration::from_millis(100);
    let question = ask("SIDE-NEVER-ADMIT");
    let request_id = question.request_id.clone();
    assert!(
        tokio::time::timeout(Duration::from_millis(20), runner.side_ask(question))
            .await
            .is_err()
    );
    assert!(runner.requests.contains_key(&request_id));
    assert_eq!(runner.auxiliary.len(), 1);
    assert_eq!(
        settled(&mut runner).await.latest_turn().status,
        SideTurnStatus::Interrupted
    );
    assert_eq!(prompts(&native).len(), 1);
    runner.close_side().await.unwrap();
}
