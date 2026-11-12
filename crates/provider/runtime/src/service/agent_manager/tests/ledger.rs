use std::time::Duration;

use super::super::ledger as hooks;
use super::*;
use crate::ports::usage_ledger::{CallOutcome, CallSource};
use crate::protocol::agent_execution::ActiveTurnBehavior;
use crate::storage::timeline::Timeline;
use crate::test_support::usage_ledger::{RecordingLedger, Write};
use provider_codex::testing::Fixture;
use provider_core::ports::usage_ledger::{RequestUsage, UsageBoundary};
use provider_core::protocol::prompt::AgentPrompt;

const AGENT: &str = "agent";

/// Manager with one Codex fake Agent whose provider calls go to a recording ledger.
struct Harness {
    /// Codex fake program and working directory; kept alive for the test.
    _fixture: Fixture,
    /// Manager that owns the Agent.
    manager: AgentManager,
    /// Durable records of the manager.
    registry: MemoryRegistry,
    /// Every ledger write of the manager.
    ledger: Arc<RecordingLedger>,
}

/// Create the harness Agent with `registration`, running the Codex fake in `mode`.
async fn harness(mode: &str, registration: AgentRegistration) -> Harness {
    let fixture = Fixture::new();
    fixture.mode(mode);
    let registry = MemoryRegistry::default();
    let ledger = Arc::new(RecordingLedger::default());
    let mut manager = AgentManager::new(Box::new(registry.clone()))
        .with_timeline(Timeline::memory().unwrap())
        .with_ledger(ledger.clone());
    manager.register_client(Box::new(fixture.client())).unwrap();
    manager
        .create(AGENT, &fixture.spec(), registration)
        .await
        .unwrap();
    Harness {
        _fixture: fixture,
        manager,
        registry,
        ledger,
    }
}

/// Poll `manager` until `done` holds.
async fn poll_until(manager: &mut AgentManager, done: impl Fn(&AgentManager) -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            manager.poll().await.unwrap();
            if done(manager) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the turn finishes");
}

/// Start a turn with `text` and return its id once it has finished.
async fn run(manager: &mut AgentManager, text: &str) -> String {
    manager.send(AGENT, text).await.unwrap();
    let turn = manager
        .active_turn(AGENT)
        .expect("an accepted turn")
        .to_owned();
    poll_until(manager, |manager| manager.active_turn(AGENT).is_none()).await;
    turn
}

/// Kind and call id of every write, in order.
fn trace(ledger: &RecordingLedger) -> Vec<(&'static str, String)> {
    ledger
        .writes()
        .into_iter()
        .map(|write| match write {
            Write::Begin(call) => ("begin", call.call_id),
            Write::Request(request) => ("request", request.call_id),
            Write::Finish(finish) => ("finish", finish.call_id),
        })
        .collect()
}

#[tokio::test]
async fn each_turn_is_one_call_that_owns_its_requests() {
    let mut harness = harness(
        "normal",
        AgentRegistration {
            workspace_id: Some("workspace-1".to_owned()),
            ..AgentRegistration::default()
        },
    )
    .await;

    let first = run(&mut harness.manager, "request-usage").await;
    let second = run(&mut harness.manager, "second").await;

    assert_ne!(first, second);
    assert_eq!(
        trace(&harness.ledger),
        [
            ("begin", first.clone()),
            ("request", first.clone()),
            ("request", first.clone()),
            ("finish", first.clone()),
            ("begin", second.clone()),
            ("finish", second.clone()),
        ]
    );
    let begins = harness.ledger.begins();
    for call in &begins {
        assert_eq!(call.agent_id, AGENT);
        assert_eq!(call.provider, "codex");
        assert_eq!(call.source, CallSource::User);
        assert_eq!(call.workspace_id.as_deref(), Some("workspace-1"));
        assert_eq!(call.model.as_deref(), Some("offline-model"));
        assert!(call.started_at > 0);
    }
    let requests = harness.ledger.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.usage.segment_id.as_str())
            .collect::<Vec<_>>(),
        [format!("codex:{first}:107"), format!("codex:{first}:150")]
    );
    assert!(requests.iter().all(|request| request.agent_id == AGENT));
    let finishes = harness.ledger.finishes();
    assert!(finishes.iter().all(|finish| {
        finish.outcome == CallOutcome::Completed && finish.token_usage_available
    }));
    assert!(begins[0].started_at <= finishes[0].ended_at);
}

#[tokio::test]
async fn steered_input_keeps_the_active_call() {
    let mut harness = harness("normal", AgentRegistration::default()).await;

    harness.manager.send(AGENT, "hang").await.unwrap();
    let turn = harness.manager.active_turn(AGENT).unwrap().to_owned();
    harness
        .manager
        .send_steering(AGENT, "request-usage")
        .await
        .unwrap();
    poll_until(&mut harness.manager, |manager| {
        manager.active_turn(AGENT).is_none()
    })
    .await;

    assert_eq!(
        trace(&harness.ledger),
        [
            ("begin", turn.clone()),
            ("request", turn.clone()),
            ("finish", turn.clone()),
        ]
    );
    let request = &harness.ledger.requests()[0];
    assert_eq!(request.usage.segment_id, format!("codex:{turn}:200"));
    assert_eq!(harness.ledger.finishes()[0].outcome, CallOutcome::Completed);
}

#[tokio::test]
async fn each_terminal_event_finishes_the_call_with_its_outcome() {
    let mut harness = harness("normal", AgentRegistration::default()).await;

    harness.manager.send(AGENT, "hang").await.unwrap();
    let cancelled = harness.manager.active_turn(AGENT).unwrap().to_owned();
    harness.manager.cancel(AGENT).await.unwrap();
    poll_until(&mut harness.manager, |manager| {
        manager.active_turn(AGENT).is_none()
    })
    .await;
    let failed = run(&mut harness.manager, "fail").await;

    let finishes = harness.ledger.finishes();
    assert_eq!(
        finishes
            .iter()
            .map(|finish| (finish.call_id.as_str(), finish.outcome))
            .collect::<Vec<_>>(),
        [
            (cancelled.as_str(), CallOutcome::Cancelled),
            (failed.as_str(), CallOutcome::Failed),
        ]
    );
    assert!(finishes.iter().all(|finish| finish.token_usage_available));
    assert_eq!(
        harness.registry.get(AGENT).unwrap().unwrap().last_status,
        AgentRuntimeStatus::Error
    );
}

#[tokio::test]
async fn a_rejected_start_is_a_finished_call_without_requests() {
    let mut harness = harness(
        "normal",
        AgentRegistration {
            labels: BTreeMap::from([("paseo.schedule-id".to_owned(), "schedule-1".to_owned())]),
            ..AgentRegistration::default()
        },
    )
    .await;

    assert_eq!(
        harness.manager.send(AGENT, "reject-before-admission").await,
        Err(AgentManagerError::Session)
    );

    let writes = harness.ledger.writes();
    let [Write::Begin(begin), Write::Finish(finish)] = writes.as_slice() else {
        panic!("expected one begun and finished call, got {writes:?}");
    };
    assert!(begin.call_id.starts_with("rejected:agent:"));
    assert_eq!(begin.source, CallSource::Schedule);
    assert_eq!(begin.model.as_deref(), Some("offline-model"));
    assert_eq!(finish.call_id, begin.call_id);
    assert_eq!(finish.outcome, CallOutcome::Rejected);
    assert_eq!(harness.manager.active_turn(AGENT), None);
}

#[tokio::test]
async fn a_native_turn_without_host_input_is_an_autonomous_call() {
    let mut harness = harness("workflows", AgentRegistration::default()).await;

    harness
        .manager
        .deliver(
            AGENT,
            &AgentPrompt::text("/goal autonomous fixture"),
            ActiveTurnBehavior::Interrupt,
        )
        .await
        .unwrap();
    poll_until(&mut harness.manager, |manager| {
        manager.active_turn(AGENT).is_none() && manager.latest_turn(AGENT).is_some()
    })
    .await;

    let turn = harness.manager.latest_turn(AGENT).unwrap().to_owned();
    assert_eq!(
        trace(&harness.ledger),
        [("begin", turn.clone()), ("finish", turn.clone())]
    );
    assert_eq!(harness.ledger.begins()[0].source, CallSource::Autonomous);
    assert_eq!(harness.ledger.finishes()[0].outcome, CallOutcome::Completed);
}

#[tokio::test]
async fn ledger_writes_never_change_the_turn() {
    for ledger in [
        Arc::new(NoopUsageLedger) as Arc<dyn UsageLedger>,
        Arc::new(RecordingLedger::default()),
    ] {
        let fixture = Fixture::new();
        let registry = MemoryRegistry::default();
        let mut manager = AgentManager::new(Box::new(registry.clone())).with_ledger(ledger.clone());
        manager.register_client(Box::new(fixture.client())).unwrap();
        manager
            .create(AGENT, &fixture.spec(), AgentRegistration::default())
            .await
            .unwrap();

        run(&mut manager, "request-usage").await;

        let record = registry.get(AGENT).unwrap().unwrap();
        assert_eq!(record.last_status, AgentRuntimeStatus::Idle, "{ledger:?}");
        assert!(record.last_error.is_none());
        assert_eq!(manager.last_message(AGENT), Some("Echo: request-usage"));
    }
}

/// Request usage reported by the fake session.
fn request_usage(segment_id: &str) -> RequestUsage {
    RequestUsage {
        segment_id: segment_id.to_owned(),
        parent: None,
        model: None,
        boundary: UsageBoundary::Turn,
        input_tokens: 1,
        output_tokens: 1,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        cache_write_1h_tokens: None,
        reasoning_tokens: None,
        cost_usd_reported: None,
        complete: true,
    }
}

#[tokio::test]
async fn sessions_without_token_usage_finish_calls_as_such_and_late_requests_are_dropped() {
    let (manager, _, client) = make_manager();
    let ledger = Arc::new(RecordingLedger::default());
    let mut manager = manager.with_ledger(ledger.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent-1", "hello").await.unwrap();
    client.0.lock().unwrap().events.extend([
        AgentTurnEvent::RequestUsage(request_usage("turn-segment")),
        AgentTurnEvent::Completed(None),
    ]);
    manager.poll().await.unwrap();

    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::RequestUsage(request_usage("late-segment")));
    manager.poll().await.unwrap();

    assert_eq!(
        trace(&ledger),
        [
            ("begin", "native-turn".to_owned()),
            ("request", "native-turn".to_owned()),
            ("finish", "native-turn".to_owned()),
        ]
    );
    assert_eq!(ledger.begins()[0].model.as_deref(), Some("gpt-6"));
    assert!(!ledger.finishes()[0].token_usage_available);
}

#[test]
fn call_source_comes_from_the_bot_owner_and_schedule_label() {
    let user = stored_record();
    let mut bot = stored_record();
    bot.internal = true;
    bot.owner = Some(serde_json::json!({"workspaceId":"workspace-1"}));
    let mut internal = stored_record();
    internal.internal = true;
    let mut scheduled = stored_record();
    scheduled
        .labels
        .insert("paseo.schedule-id".to_owned(), "schedule-1".to_owned());

    assert_eq!(hooks::source(&user), CallSource::User);
    assert_eq!(hooks::source(&bot), CallSource::Bot);
    assert_eq!(hooks::source(&internal), CallSource::User);
    assert_eq!(hooks::source(&scheduled), CallSource::Schedule);
}

#[test]
fn call_model_prefers_the_runtime_model_over_the_configured_one() {
    let config = StoredAgentConfig {
        model: Some("configured".to_owned()),
        ..StoredAgentConfig::default()
    };
    let mut runtime = StoredAgentRuntimeInfo {
        provider: "codex".to_owned(),
        session_id: None,
        model: Some("runtime".to_owned()),
        thinking_option_id: None,
        mode_id: None,
        extra: None,
    };

    assert_eq!(
        hooks::model(Some(&runtime), Some(&config)).as_deref(),
        Some("runtime")
    );
    runtime.model = None;
    assert_eq!(
        hooks::model(Some(&runtime), Some(&config)).as_deref(),
        Some("configured")
    );
    assert_eq!(hooks::model(None, None), None);
}

/// How a test closes the live session while its turn is still running.
#[derive(Debug, Clone, Copy)]
enum Closing {
    /// A direct close, as kill, refresh and restore use.
    Close,
    /// Archiving the durable record, which `reconcile` closes.
    Archive,
    /// Host shutdown.
    Shutdown,
}

#[tokio::test]
async fn closing_a_session_mid_turn_interrupts_its_call() {
    for closing in [Closing::Close, Closing::Archive, Closing::Shutdown] {
        let mut harness = harness("normal", AgentRegistration::default()).await;
        harness.manager.send(AGENT, "hang").await.unwrap();
        let turn = harness.manager.active_turn(AGENT).unwrap().to_owned();

        match closing {
            Closing::Close => harness.manager.close(AGENT).await.unwrap(),
            Closing::Archive => {
                harness
                    .registry
                    .update(AGENT, &|current| {
                        let mut next = current.clone();
                        next.archived_at = Some("2026-11-11T00:00:00.000Z".to_owned());
                        next
                    })
                    .unwrap();
                harness.manager.reconcile().await.unwrap();
            }
            Closing::Shutdown => harness.manager.close_all().await.unwrap(),
        }

        assert!(
            harness.manager.live_snapshot(AGENT).is_none(),
            "{closing:?}"
        );
        assert_eq!(
            trace(&harness.ledger),
            [("begin", turn.clone()), ("finish", turn.clone())],
            "{closing:?}"
        );
        let finish = &harness.ledger.finishes()[0];
        assert_eq!(finish.outcome, CallOutcome::Interrupted, "{closing:?}");
        assert!(finish.token_usage_available);
    }
}

#[tokio::test]
async fn closing_an_idle_session_finishes_no_call() {
    let mut harness = harness("normal", AgentRegistration::default()).await;
    let turn = run(&mut harness.manager, "done").await;

    harness.manager.close(AGENT).await.unwrap();

    assert_eq!(
        trace(&harness.ledger),
        [("begin", turn.clone()), ("finish", turn)]
    );
    assert_eq!(harness.ledger.finishes()[0].outcome, CallOutcome::Completed);
}
