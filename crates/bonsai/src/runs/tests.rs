use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use model::ErrorCode;
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use super::{Coordinator, Input, WritePolicy};
use crate::event::{Body, Event, Level, Origin, RejectCode, TurnState};
use crate::hello::{self, Model, Offer, Provider};
use crate::outbox::{Outbox, Outgoing};
use crate::ports::Project;
use crate::session::{CANCEL_GRACE, Notice};
use crate::settings::{self, Mode};
use crate::store::{RunRecord, StatusUpdate, Store};
use crate::testing::{MockHost, dispatch};
use crate::translate::uuid_of;
use crate::wire::{
    Answer, Cursor, Dispatch, Execution, Inbound, Person, REASON_DETAIL_BYTES, ReasonCode,
    RunState, Status, Welcome,
};

const RUN: &str = "r_0123456789abcdef0123456789abcdef";
const PROJECT: &str = "prj_0123456789abcdef";
const WRAPUP: &str = "WRAPUP-MARKER";
const INPUT: &str = "00112233445566778899aabbccddeeff";
const LOOPBACK_MCP: &str = "http://localhost:8860/mcp";
const REMOTE_MCP: &str = "https://bonsai.example.com/mcp";

/// Runtime on the same machine as its Bonsai: the dispatch MCP URL is injected.
const LOCAL: WritePolicy = WritePolicy { loopback: true };
/// Remote Bonsai: no Bonsai tools and no Bonsai writes.
const REMOTE: WritePolicy = WritePolicy { loopback: false };

fn modes(provider: &str) -> Vec<Mode> {
    let ids: &[&str] = if provider == "codex" {
        &["read-only", "auto", "auto-review", "full-access"]
    } else {
        &["default", "acceptEdits", "plan", "bypassPermissions"]
    };
    ids.iter()
        .map(|id| Mode {
            id: (*id).to_owned(),
            label: (*id).to_owned(),
        })
        .collect()
}

fn provider(id: &str, bonsai_write: bool) -> Provider {
    Provider {
        id: id.to_owned(),
        models: ["default", "sonnet"]
            .iter()
            .map(|model| Model {
                id: (*model).to_owned(),
                label: (*model).to_owned(),
            })
            .collect(),
        approvals: settings::mode_asks(id, settings::default_mode(id)),
        bonsai_write,
        settings: settings::declarations(id, &modes(id), bonsai_write),
        traits: HashMap::new(),
        default_model: None,
    }
}

/// What `hello::discover` would announce under `policy` for these providers.
fn offer(policy: WritePolicy, providers: &[&str]) -> Offer {
    Offer {
        name: "test-machine".to_owned(),
        providers: providers
            .iter()
            .map(|id| provider(id, policy.bonsai_write(id)))
            .collect(),
        projects: hello::announce(vec![Project {
            id: PROJECT.to_owned(),
            name: "repo".to_owned(),
            root: "/tmp/repo".to_owned(),
            remote_url: None,
            branch: Some("main".to_owned()),
        }]),
    }
}

fn dispatch_for(run_id: &str) -> Dispatch {
    let mut frame = dispatch(run_id);
    frame.wrapup = WRAPUP.to_owned();
    frame
}

fn settings_of(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

fn agent_of(run_id: &str) -> String {
    uuid_of(run_id.trim_start_matches("r_"))
}

fn member() -> Person {
    Person {
        id: "github:900002".to_owned(),
        login: Some("sandbox-member".to_owned()),
    }
}

fn send_frame(run_id: &str, input_id: &str) -> Inbound {
    Inbound::Send {
        run_id: run_id.to_owned(),
        by: member(),
        input_id: input_id.to_owned(),
        text: "please also run the tests".to_owned(),
    }
}

fn answer_frame(run_id: &str, ask_id: &str) -> Inbound {
    Inbound::Answer(Answer {
        run_id: run_id.to_owned(),
        by: member(),
        ask_id: ask_id.to_owned(),
        option_id: "allow".to_owned(),
        answers: None,
        note: None,
    })
}

fn claude_execution() -> Execution {
    Execution {
        provider: "claude".to_owned(),
        model: Some("sonnet".to_owned()),
        approvals: true,
        bonsai_write: true,
    }
}

fn update(status: RunState) -> StatusUpdate {
    StatusUpdate {
        status,
        reason_code: None,
        reason_detail: None,
        final_text: None,
        at: 99,
    }
}

fn event_json(seq: u64, body: Body) -> String {
    Event { seq, at: 1, body }
        .to_json()
        .expect("event serializes")
}

/// Record a claimed run with an execution directly in the store, as a previous dispatch would.
fn seed(store: &Store, run_id: &str, at: i64) {
    store
        .insert_run(&dispatch_for(run_id), "e1", at)
        .expect("run inserts");
    store
        .set_execution(run_id, &claude_execution())
        .expect("execution records");
}

/// A claimed run whose log holds the dispatch input and an open turn, as before `agent.create`.
fn seed_open(store: &Store, run_id: &str) {
    seed(store, run_id, 5);
    let events = [
        event_json(
            0,
            Body::Input {
                id: "dispatch-01234567".to_owned(),
                text: "list the files".to_owned(),
                by: "github:900002".to_owned(),
                login: None,
                origin: Origin::Dispatch,
            },
        ),
        event_json(
            1,
            Body::Turn {
                state: TurnState::Started,
                reason: None,
            },
        ),
    ];
    store
        .append(run_id, "e1", 0, &events, None)
        .expect("log appends");
}

/// Let spawned session tasks run until they wait; the paused clock moves only 1 ms.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

fn single_status(sent: Vec<Outgoing>) -> Status {
    assert_eq!(sent.len(), 1, "exactly one frame expected: {sent:?}");
    match sent.into_iter().next() {
        Some(Outgoing::Status(status)) => status,
        other => panic!("expected a status frame, got {other:?}"),
    }
}

fn states(statuses: &[Status]) -> Vec<RunState> {
    statuses.iter().map(|status| status.status).collect()
}

struct Harness {
    mock: MockHost,
    store: Store,
    coordinator: Coordinator,
    frames: mpsc::UnboundedReceiver<Outgoing>,
    notices: mpsc::UnboundedReceiver<Notice>,
    /// Runs credited with a free status, in order (kept out of `sent`).
    credits: Vec<String>,
}

impl Harness {
    fn new() -> Self {
        Self::with(LOCAL, &["claude"])
    }

    fn with(policy: WritePolicy, providers: &[&str]) -> Self {
        let mock = MockHost::new();
        let store = Store::memory().expect("in-memory store opens");
        let outbox = Outbox::default();
        let (sender, frames) = mpsc::unbounded_channel();
        outbox.attach(sender);
        let (notice_sender, notices) = mpsc::unbounded_channel();
        let coordinator = Coordinator::new(
            mock.host(),
            store.clone(),
            outbox,
            Arc::new(RwLock::new(offer(policy, providers))),
            notice_sender,
            policy,
        );
        Self {
            mock,
            store,
            coordinator,
            frames,
            notices,
            credits: Vec::new(),
        }
    }

    fn frame(&mut self, frame: Inbound) {
        self.coordinator.handle(Input::Frame(frame));
    }

    fn dispatch(&mut self, frame: Dispatch) {
        self.frame(Inbound::Dispatch(Box::new(frame)));
    }

    fn sent(&mut self) -> Vec<Outgoing> {
        let mut sent = Vec::new();
        while let Ok(outgoing) = self.frames.try_recv() {
            match outgoing {
                Outgoing::Credit { run_id } => self.credits.push(run_id),
                other => sent.push(other),
            }
        }
        sent
    }

    fn statuses(&mut self) -> Vec<Status> {
        self.sent()
            .into_iter()
            .filter_map(|outgoing| match outgoing {
                Outgoing::Status(status) => Some(status),
                _ => None,
            })
            .collect()
    }

    fn creates_succeed(&self) {
        self.mock.executor.always(
            "agent.create.request",
            Ok(json!({"agentId": agent_of(RUN), "error": null,
                      "agent": {"runtimeInfo": {"model": "claude-sonnet-4-5"}}})),
        );
    }

    fn record(&self, run_id: &str) -> RunRecord {
        self.store
            .run(run_id)
            .expect("store reads")
            .expect("run is recorded")
    }

    fn log(&self, run_id: &str) -> Vec<(u64, Body)> {
        let record = self.record(run_id);
        self.store
            .events(run_id, &record.epoch, 0, record.next_seq)
            .expect("log reads")
            .iter()
            .map(|json| {
                let event: Event = serde_json::from_str(json).expect("logged event parses");
                (event.seq, event.body)
            })
            .collect()
    }
}

// ---------------------------------------------------------------- run.query

#[tokio::test]
async fn query_of_unknown_run_reports_run_unknown_without_execution() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.frame(Inbound::Query {
        run_id: RUN.to_owned(),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.run_id, RUN);
    assert_eq!(status.status, RunState::Failed);
    assert_eq!(status.reason_code, Some(ReasonCode::RunUnknown));
    assert_eq!(status.execution, None);
    assert_eq!(status.final_text, None);
    assert!(status.at > 0);
    assert!(harness.mock.executor.calls().is_empty());
    assert_eq!(harness.store.run(RUN).expect("store reads"), None);
}

#[tokio::test]
async fn query_of_tombstoned_run_reports_cancelled() {
    // Arrange
    let mut harness = Harness::new();
    harness.store.bury(RUN, 1).expect("tombstone records");

    // Act
    harness.frame(Inbound::Query {
        run_id: RUN.to_owned(),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Cancelled);
    assert_eq!(status.reason_code, None);
    assert_eq!(status.execution, None);
}

#[tokio::test]
async fn query_of_known_run_reports_the_recorded_status_with_execution() {
    // Arrange
    let completed = StatusUpdate {
        final_text: Some("done: three files".to_owned()),
        ..update(RunState::Completed)
    };
    let failed = StatusUpdate {
        reason_code: Some(ReasonCode::ProviderError),
        reason_detail: Some("the provider exited".to_owned()),
        ..update(RunState::Failed)
    };
    let cases = [
        ("claimed", None),
        ("running", Some(update(RunState::Running))),
        ("completed", Some(completed)),
        ("failed", Some(failed)),
    ];

    for (name, advance) in cases {
        let mut harness = Harness::new();
        seed(&harness.store, RUN, 1234);
        if let Some(advance) = &advance {
            harness.store.advance(RUN, advance).expect("state advances");
        }

        // Act
        harness.frame(Inbound::Query {
            run_id: RUN.to_owned(),
        });

        // Assert
        let expected = Status {
            kind: "run.status",
            run_id: RUN.to_owned(),
            status: advance
                .as_ref()
                .map_or(RunState::Claimed, |update| update.status),
            at: advance.as_ref().map_or(1234, |update| update.at),
            execution: Some(claude_execution()),
            reason_code: advance.as_ref().and_then(|update| update.reason_code),
            reason_detail: advance
                .as_ref()
                .and_then(|update| update.reason_detail.clone()),
            final_text: advance
                .as_ref()
                .and_then(|update| update.final_text.clone()),
        };
        assert_eq!(single_status(harness.sent()), expected, "{name}");
        assert!(harness.mock.executor.calls().is_empty(), "{name}");
    }
}

// ------------------------------------------------------------- run.dispatch

#[tokio::test(start_paused = true)]
async fn dispatch_records_the_run_and_reports_claimed_before_any_agent_call() {
    // Arrange
    let mut harness = Harness::new();
    let mut frame = dispatch_for(RUN);
    frame.model = Some("sonnet".to_owned());

    // Act
    harness.dispatch(frame);

    // Assert
    let claimed = single_status(harness.sent());
    assert_eq!(claimed.status, RunState::Claimed);
    assert_eq!(claimed.execution, Some(claude_execution()));
    assert_eq!(
        (
            claimed.reason_code,
            claimed.reason_detail,
            claimed.final_text
        ),
        (None, None, None)
    );
    assert!(claimed.at > 0);
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Claimed);
    assert_eq!(record.execution, Some(claude_execution()));
    assert_eq!(record.mode.as_deref(), Some("default"));
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test(start_paused = true)]
async fn dispatch_starts_one_agent_and_reports_running_with_the_resolved_model() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();

    // Act
    harness.dispatch(dispatch_for(RUN));
    settle().await;

    // Assert
    let statuses = harness.statuses();
    assert_eq!(states(&statuses), [RunState::Claimed, RunState::Running]);
    let claimed = statuses[0]
        .execution
        .clone()
        .expect("claimed carries execution");
    let running = statuses[1]
        .execution
        .clone()
        .expect("running carries execution");
    assert_eq!((claimed.provider.as_str(), claimed.model), ("claude", None));
    assert_eq!(running.model.as_deref(), Some("claude-sonnet-4-5"));
    let creates = harness.mock.executor.calls_of("agent.create.request");
    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0]["agentId"], agent_of(RUN));
    assert_eq!(creates[0]["idempotencyKey"], RUN);
    assert_eq!(creates[0]["config"]["modeId"], "default");
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Running);
    assert_eq!(record.agent_id, Some(agent_of(RUN)));
}

#[tokio::test(start_paused = true)]
async fn duplicate_dispatch_resends_the_status_and_never_starts_a_second_session() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();

    // Act
    harness.dispatch(dispatch_for(RUN));
    harness.dispatch(dispatch_for(RUN));
    let early = harness.statuses();
    settle().await;
    let _ = harness.sent();
    harness.dispatch(dispatch_for(RUN));
    let late = single_status(harness.sent());
    settle().await;

    // Assert
    assert_eq!(states(&early), [RunState::Claimed, RunState::Claimed]);
    assert!(early.iter().all(|status| status.execution.is_some()));
    assert_eq!(late.status, RunState::Running);
    assert!(late.execution.is_some());
    assert_eq!(
        harness.mock.executor.calls_of("agent.create.request").len(),
        1
    );
    assert_eq!(harness.mock.observer.observed(), [agent_of(RUN)]);
}

#[tokio::test(start_paused = true)]
async fn dispatch_of_tombstoned_run_reports_cancelled_and_never_starts() {
    // Arrange
    let mut harness = Harness::new();
    harness.store.bury(RUN, 1).expect("tombstone records");

    // Act
    harness.dispatch(dispatch_for(RUN));
    settle().await;

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Cancelled);
    assert_eq!((status.execution, status.reason_code), (None, None));
    assert_eq!(harness.store.run(RUN).expect("store reads"), None);
    assert!(harness.mock.executor.calls().is_empty());
    assert!(harness.mock.observer.observed().is_empty());
}

// --------------------------------------------------------------- run.cancel

#[tokio::test(start_paused = true)]
async fn cancel_of_unknown_run_buries_it_and_a_later_dispatch_is_cancelled() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });
    let cancelled = single_status(harness.sent());
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    let redispatched = single_status(harness.sent());
    harness.frame(Inbound::Query {
        run_id: RUN.to_owned(),
    });
    let queried = single_status(harness.sent());

    // Assert
    for status in [cancelled, redispatched, queried] {
        assert_eq!(status.status, RunState::Cancelled);
        assert_eq!((status.execution, status.reason_code), (None, None));
    }
    assert!(harness.store.is_buried(RUN).expect("store reads"));
    assert_eq!(harness.store.run(RUN).expect("store reads"), None);
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn cancel_of_terminal_run_resends_the_terminal_status() {
    // Arrange
    let completed = StatusUpdate {
        final_text: Some("done".to_owned()),
        ..update(RunState::Completed)
    };
    let failed = StatusUpdate {
        reason_code: Some(ReasonCode::ProviderError),
        reason_detail: Some("boom".to_owned()),
        ..update(RunState::Failed)
    };
    for terminal in [completed, failed, update(RunState::Cancelled)] {
        let mut harness = Harness::new();
        seed(&harness.store, RUN, 5);
        harness
            .store
            .advance(RUN, &terminal)
            .expect("state advances");

        // Act
        harness.frame(Inbound::Cancel {
            run_id: RUN.to_owned(),
        });

        // Assert
        let expected = Status {
            kind: "run.status",
            run_id: RUN.to_owned(),
            status: terminal.status,
            at: terminal.at,
            execution: Some(claude_execution()),
            reason_code: terminal.reason_code,
            reason_detail: terminal.reason_detail.clone(),
            final_text: terminal.final_text.clone(),
        };
        assert_eq!(single_status(harness.sent()), expected);
        assert!(!harness.record(RUN).cancel_requested);
        assert!(harness.mock.executor.calls().is_empty());
    }
}

#[tokio::test]
async fn cancel_of_claimed_run_without_a_session_reports_cancelled() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);

    // Act
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Cancelled);
    assert_eq!(status.execution, Some(claude_execution()));
    assert_eq!(status.reason_code, None);
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Cancelled);
    assert!(record.cancel_requested);
}

#[tokio::test]
async fn cancel_of_idle_running_run_without_a_session_reports_completed() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    harness
        .store
        .set_agent(RUN, &agent_of(RUN))
        .expect("agent records");
    harness
        .store
        .advance(RUN, &update(RunState::Running))
        .expect("state advances");

    // Act
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Completed);
    assert_eq!(status.execution, Some(claude_execution()));
    assert_eq!(harness.record(RUN).status, RunState::Completed);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_agent_creation_waits_for_the_provider_to_confirm() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();
    let gate = harness.mock.executor.hold("agent.create.request");
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    let claimed = harness.statuses();

    // Act
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });
    let during_create = harness.sent();
    gate.add_permits(1);
    settle().await;
    let after_create = harness.statuses();
    let interrupts = harness.mock.executor.calls_of("agent.cancel.request");
    let confirmed = harness.mock.observer.emit(
        &agent_of(RUN),
        "agent_stream",
        json!({"event": {"type": "turn_canceled", "reason": "interrupted"}}),
    );
    settle().await;
    let stopped = harness.statuses();

    // Assert
    assert_eq!(states(&claimed), [RunState::Claimed]);
    assert!(
        during_create.is_empty(),
        "no frame while create runs: {during_create:?}"
    );
    assert_eq!(states(&after_create), [RunState::Running]);
    assert_eq!(interrupts, [json!({"agentId": agent_of(RUN)})]);
    assert!(confirmed);
    assert_eq!(states(&stopped), [RunState::Cancelled]);
    assert!(stopped[0].execution.is_some());
    assert_eq!(
        harness
            .mock
            .executor
            .calls_of("agent.archive.request")
            .len(),
        1
    );
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Cancelled);
    assert!(record.cancel_requested);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_agent_creation_force_stops_after_the_grace_period() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();
    let gate = harness.mock.executor.hold("agent.create.request");
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });
    gate.add_permits(1);
    settle().await;
    let _ = harness.sent();

    // Act
    tokio::time::sleep(CANCEL_GRACE.saturating_sub(Duration::from_secs(1))).await;
    let before_grace = harness.statuses();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let after_grace = harness.statuses();

    // Assert
    assert!(
        before_grace.is_empty(),
        "no terminal state before the grace period"
    );
    assert_eq!(states(&after_grace), [RunState::Cancelled]);
    assert_eq!(
        harness
            .mock
            .executor
            .calls_of("agent.archive.request")
            .len(),
        1
    );
    let log = harness.log(RUN);
    assert!(log.iter().any(|(_, body)| *body
        == Body::Turn {
            state: TurnState::Aborted,
            reason: Some("cancelled".to_owned()),
        }));
    assert_eq!(
        log.last().map(|(_, body)| body.clone()),
        Some(Body::Closed {
            reason: Some("cancelled".to_owned()),
        })
    );
}

// --------------------------------------------------------- dispatch checks

type Mutation = fn(&mut Dispatch);

/// Dispatch `frame`, then query and re-dispatch it: all three must report the same failure,
/// with no execution, nothing executed and the run recorded.
async fn assert_fails_without_execution(
    mut harness: Harness,
    frame: Dispatch,
    code: ReasonCode,
    detail: &str,
) {
    harness.dispatch(frame.clone());
    settle().await;
    let first = single_status(harness.sent());
    harness.frame(Inbound::Query {
        run_id: RUN.to_owned(),
    });
    let queried = single_status(harness.sent());
    harness.dispatch(frame);
    settle().await;
    let repeated = single_status(harness.sent());

    assert_eq!(first.status, RunState::Failed, "{detail}");
    assert_eq!(first.reason_code, Some(code), "{detail}");
    assert!(
        first
            .reason_detail
            .as_deref()
            .is_some_and(|text| text.contains(detail)),
        "reason_detail names {detail}: {:?}",
        first.reason_detail
    );
    assert_eq!(first.execution, None, "{detail}");
    for again in [queried, repeated] {
        assert_eq!(
            (again.status, again.reason_code, again.execution),
            (RunState::Failed, Some(code), None),
            "{detail}"
        );
        assert_eq!(again.reason_detail, first.reason_detail, "{detail}");
    }
    assert!(harness.mock.executor.calls().is_empty(), "{detail}");
    assert!(harness.mock.observer.observed().is_empty(), "{detail}");
    let record = harness.record(RUN);
    assert_eq!(
        (record.status, record.reason_code, record.execution),
        (RunState::Failed, Some(code), None),
        "{detail}"
    );
}

#[tokio::test(start_paused = true)]
async fn invalid_dispatch_is_recorded_then_fails_without_execution() {
    // Arrange
    let cases: [(Mutation, ReasonCode, &str); 8] = [
        (
            |frame| frame.session = "bonsai.session/2".to_owned(),
            ReasonCode::Rejected,
            "bonsai.session/2",
        ),
        (
            |frame| frame.project.id = "prj_ffffffffffffffff".to_owned(),
            ReasonCode::ProjectUnavailable,
            "项目",
        ),
        (
            |frame| frame.provider = Some("gemini".to_owned()),
            ReasonCode::ProviderUnavailable,
            "provider",
        ),
        (
            |frame| frame.provider = Some("codex".to_owned()),
            ReasonCode::ProviderUnavailable,
            "provider",
        ),
        (
            |frame| frame.model = Some("opus-9".to_owned()),
            ReasonCode::ProviderUnavailable,
            "opus-9",
        ),
        (
            |frame| frame.settings = settings_of(json!({"turbo": true})),
            ReasonCode::Rejected,
            "turbo",
        ),
        (
            |frame| frame.settings = settings_of(json!({"permission_mode": "yolo"})),
            ReasonCode::Rejected,
            "permission_mode",
        ),
        (
            |frame| frame.settings = settings_of(json!({"fast_mode": "yes"})),
            ReasonCode::Rejected,
            "fast_mode",
        ),
    ];

    for (mutate, code, detail) in cases {
        let mut frame = dispatch_for(RUN);
        mutate(&mut frame);

        // Act and Assert
        assert_fails_without_execution(Harness::new(), frame, code, detail).await;
    }
}

#[tokio::test(start_paused = true)]
async fn codex_unattended_choice_that_still_asks_is_rejected() {
    // Arrange
    let harness = Harness::with(LOCAL, &["claude", "codex"]);
    let mut frame = dispatch_for(RUN);
    frame.provider = Some("codex".to_owned());
    frame.settings = settings_of(
        json!({"permission_mode": "full-access", "approval_policy": "on-request", "sandbox_mode": "read-only"}),
    );

    // Act and Assert
    assert_fails_without_execution(
        harness,
        frame,
        ReasonCode::Rejected,
        "permission_mode=full-access",
    )
    .await;
}

#[tokio::test(start_paused = true)]
async fn bonsai_preapprove_is_undeclared_when_writes_are_off() {
    // Arrange
    let harness = Harness::with(REMOTE, &["claude"]);
    let mut frame = dispatch_for(RUN);
    frame.settings = settings_of(json!({"bonsai_preapprove": true}));

    // Act and Assert
    assert_fails_without_execution(harness, frame, ReasonCode::Rejected, "bonsai_preapprove").await;
}

#[tokio::test(start_paused = true)]
async fn rejection_detail_is_cut_on_a_code_point_within_the_protocol_limit() {
    // Arrange
    let mut harness = Harness::new();
    let key = "设".repeat(1000);
    let mut frame = dispatch_for(RUN);
    frame.settings.insert(key, Value::Bool(true));

    // Act
    harness.dispatch(frame);

    // Assert
    let status = single_status(harness.sent());
    let detail = status.reason_detail.expect("rejection has a detail");
    assert!(
        detail.len() <= REASON_DETAIL_BYTES,
        "{} bytes",
        detail.len()
    );
    assert!(detail.len() > REASON_DETAIL_BYTES - 4);
    assert!(detail.starts_with("不认识的设定:设"));
    assert!(detail.ends_with('设'));
    let stored = harness.record(RUN).reason_detail.expect("detail is stored");
    assert_eq!(stored, detail);
}

#[tokio::test(start_paused = true)]
async fn claimed_execution_reports_approvals_as_the_settings_resolve() {
    // Arrange
    let cases = [
        ("claude", json!({}), true),
        ("claude", json!({"permission_mode": "default"}), true),
        ("claude", json!({"permission_mode": "acceptEdits"}), true),
        ("claude", json!({"permission_mode": "plan"}), true),
        (
            "claude",
            json!({"permission_mode": "bypassPermissions"}),
            false,
        ),
        ("codex", json!({}), true),
        ("codex", json!({"sandbox_mode": "read-only"}), true),
        ("codex", json!({"permission_mode": "full-access"}), false),
        ("codex", json!({"permission_mode": "read-only"}), false),
        ("codex", json!({"permission_mode": "auto-review"}), false),
        ("codex", json!({"approval_policy": "never"}), false),
    ];

    for (provider, filled, approvals) in cases {
        let mut harness = Harness::with(LOCAL, &["claude", "codex"]);
        let mut frame = dispatch_for(RUN);
        frame.provider = Some(provider.to_owned());
        frame.settings = settings_of(filled.clone());

        // Act
        harness.dispatch(frame);

        // Assert
        let claimed = single_status(harness.sent());
        assert_eq!(claimed.status, RunState::Claimed, "{provider} {filled}");
        let execution = claimed.execution.expect("claimed carries execution");
        assert_eq!(execution.provider, provider);
        assert_eq!(execution.approvals, approvals, "{provider} {filled}");
        let mode = filled["permission_mode"]
            .as_str()
            .unwrap_or_else(|| settings::default_mode(provider));
        assert_eq!(harness.record(RUN).mode.as_deref(), Some(mode));
    }
}

#[tokio::test(start_paused = true)]
async fn bonsai_write_is_true_only_when_a_loopback_mcp_url_is_injected() {
    // Arrange
    let cases = [
        (LOCAL, "claude", LOOPBACK_MCP, true),
        (LOCAL, "claude", "http://127.0.0.1:8860/mcp", true),
        (LOCAL, "claude", "http://[::1]:8860/mcp", true),
        (LOCAL, "claude", REMOTE_MCP, false),
        (LOCAL, "claude", "not a url", false),
        (LOCAL, "codex", LOOPBACK_MCP, true),
        (LOCAL, "codex", REMOTE_MCP, false),
        (REMOTE, "claude", LOOPBACK_MCP, false),
        (REMOTE, "codex", LOOPBACK_MCP, false),
        (REMOTE, "claude", REMOTE_MCP, false),
    ];

    for (policy, provider, url, expected) in cases {
        let mut harness = Harness::with(policy, &["claude", "codex"]);
        let mut frame = dispatch_for(RUN);
        frame.provider = Some(provider.to_owned());
        frame.bonsai.mcp_url = url.to_owned();

        // Act
        harness.dispatch(frame);
        let claimed = single_status(harness.sent());
        settle().await;

        // Assert
        let case = format!("{policy:?} {provider} {url}");
        let execution = claimed.execution.expect("claimed carries execution");
        assert_eq!(execution.bonsai_write, expected, "{case}");
        let creates = harness.mock.executor.calls_of("agent.create.request");
        assert_eq!(creates.len(), 1, "{case}");
        let config = &creates[0]["config"];
        assert_eq!(
            config["mcpServers"][settings::BONSAI_SERVER]["url"].as_str(),
            expected.then_some(url),
            "{case}"
        );
        let prompt = config["systemPrompt"]
            .as_str()
            .expect("system prompt is set");
        assert_eq!(
            prompt.contains(WRAPUP),
            expected,
            "wrapup only with writes: {case}"
        );
        assert_eq!(
            config["providerOptions"]["strictMcp"], true,
            "strict MCP: {case}"
        );
    }
}

// ------------------------------------------------------------ session.* frames

#[tokio::test]
async fn subscribe_is_handed_to_the_writer_unchanged() {
    // Arrange
    let mut harness = Harness::new();
    let cursor = Some(Cursor {
        epoch: "e1".to_owned(),
        seq: 4,
    });

    for after in [None, cursor] {
        // Act
        harness.frame(Inbound::Subscribe {
            run_id: RUN.to_owned(),
            sub: "0123456789abcdef0123456789abcdef".to_owned(),
            after: after.clone(),
        });

        // Assert
        assert_eq!(
            harness.sent(),
            [Outgoing::Answer {
                run_id: RUN.to_owned(),
                sub: "0123456789abcdef0123456789abcdef".to_owned(),
                after,
            }]
        );
    }
}

#[tokio::test]
async fn send_and_answer_for_an_unknown_run_report_unavailable_with_their_ref() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.frame(send_frame(RUN, INPUT));
    let sent_reply = harness.sent();
    harness.frame(answer_frame(RUN, "ask-7"));
    let answer_reply = harness.sent();

    // Assert
    assert_eq!(
        sent_reply,
        [Outgoing::Unavailable {
            run_id: RUN.to_owned(),
            reference: INPUT.to_owned(),
        }]
    );
    assert_eq!(
        answer_reply,
        [Outgoing::Unavailable {
            run_id: RUN.to_owned(),
            reference: "ask-7".to_owned(),
        }]
    );
    assert_eq!(harness.store.run(RUN).expect("store reads"), None);
    assert!(harness.store.inputs(RUN).expect("store reads").is_empty());
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn interrupt_without_a_live_session_sends_nothing() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, "r_ffffffffffffffffffffffffffffffff", 5);

    for run_id in [RUN, "r_ffffffffffffffffffffffffffffffff"] {
        // Act
        harness.frame(Inbound::Interrupt {
            run_id: run_id.to_owned(),
        });

        // Assert
        assert!(harness.sent().is_empty(), "{run_id}");
    }
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn send_to_a_finished_run_is_rejected_once_in_its_log() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    harness
        .store
        .advance(RUN, &update(RunState::Completed))
        .expect("state advances");
    let closed = Body::Closed {
        reason: Some("idle".to_owned()),
    };
    harness
        .store
        .append(RUN, "e1", 0, &[event_json(0, closed.clone())], None)
        .expect("log appends");

    // Act
    harness.frame(send_frame(RUN, INPUT));
    let first = harness.sent();
    harness.frame(send_frame(RUN, INPUT));
    let repeated = harness.sent();

    // Assert
    assert_eq!(
        first,
        [Outgoing::Live {
            run_id: RUN.to_owned(),
        }]
    );
    assert!(
        repeated.is_empty(),
        "the same input_id is ignored: {repeated:?}"
    );
    let rejected = Body::InputRejected {
        id: INPUT.to_owned(),
        code: RejectCode::Closed,
        reason: None,
    };
    assert_eq!(harness.log(RUN), [(0, closed), (1, rejected)]);
    let inputs = harness.store.inputs(RUN).expect("store reads");
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].state, "rejected");
    assert_eq!(inputs[0].message_id, uuid_of(INPUT));
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn answer_for_a_known_run_without_a_session_is_ignored() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    harness
        .store
        .advance(RUN, &update(RunState::Completed))
        .expect("state advances");

    // Act
    harness.frame(answer_frame(RUN, "ask-7"));

    // Assert
    assert!(harness.sent().is_empty());
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn welcome_and_unknown_frames_are_ignored() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.frame(Inbound::Unknown);
    harness.frame(Inbound::Welcome(Welcome { owner: member() }));

    // Assert
    assert!(harness.sent().is_empty());
    assert!(harness.mock.executor.calls().is_empty());
}

// ------------------------------------------------------------ malformed frames

#[tokio::test]
async fn malformed_dispatch_naming_a_new_run_fails_as_rejected() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.coordinator.handle(Input::Malformed {
        kind: "run.dispatch".to_owned(),
        run_id: Some(RUN.to_owned()),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Failed);
    assert_eq!(status.reason_code, Some(ReasonCode::Rejected));
    assert!(status.reason_detail.is_some());
    assert_eq!(status.execution, None);
    assert!(harness.mock.executor.calls().is_empty());
}

#[tokio::test]
async fn malformed_frames_without_a_usable_dispatch_run_are_ignored() {
    // Arrange
    let mut harness = Harness::new();
    let long = format!("r_{}", "0".repeat(63));
    let other_prefix = format!("x_{}", "0".repeat(32));
    let cases = [
        ("run.dispatch", None),
        ("run.dispatch", Some("r_has a space")),
        ("run.dispatch", Some(long.as_str())),
        ("run.dispatch", Some("r_1")),
        ("run.dispatch", Some("r_0123456789ABCDEF0123456789ABCDEF")),
        ("run.dispatch", Some(other_prefix.as_str())),
        ("run.query", Some(RUN)),
        ("session.send", Some(RUN)),
    ];

    for (kind, run_id) in cases {
        // Act
        harness.coordinator.handle(Input::Malformed {
            kind: kind.to_owned(),
            run_id: run_id.map(str::to_owned),
        });

        // Assert
        assert!(harness.sent().is_empty(), "{kind} {run_id:?}");
    }
}

#[tokio::test]
async fn malformed_dispatch_of_a_known_run_resends_its_status() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    harness
        .store
        .advance(RUN, &update(RunState::Running))
        .expect("state advances");

    // Act
    harness.coordinator.handle(Input::Malformed {
        kind: "run.dispatch".to_owned(),
        run_id: Some(RUN.to_owned()),
    });

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Running);
    assert_eq!(status.execution, Some(claude_execution()));
    assert_eq!(status.reason_code, None);
}

// ------------------------------------------------------------------ recover

/// An Agent snapshot shaped like AIT's: labels are always present.
fn live_agent(run_id: &str) -> Value {
    json!({"agent": {"id": agent_of(run_id), "currentModeId": "default", "pendingPermissions": [],
                     "labels": {"bonsai.run": run_id, "bonsai.space": "sandbox"}}})
}

#[tokio::test(start_paused = true)]
async fn recover_fails_a_run_whose_agent_is_gone_and_closes_its_log() {
    // Arrange
    let gone = [Ok(json!({"agent": null})), Err(ErrorCode::AgentNotFound)];

    for answer in gone {
        let mut harness = Harness::new();
        seed_open(&harness.store, RUN);
        harness
            .mock
            .executor
            .respond("agent.get.request", answer.clone());

        // Act
        harness.coordinator.recover().await;
        settle().await;

        // Assert
        let status = single_status(harness.sent());
        assert_eq!(status.status, RunState::Failed, "{answer:?}");
        assert_eq!(
            status.reason_code,
            Some(ReasonCode::SessionLost),
            "{answer:?}"
        );
        assert!(status.reason_detail.is_some());
        assert_eq!(status.execution, Some(claude_execution()), "{answer:?}");
        let record = harness.record(RUN);
        assert_eq!(
            (record.status, record.reason_code),
            (RunState::Failed, Some(ReasonCode::SessionLost))
        );
        assert_eq!(
            harness.log(RUN).last(),
            Some(&(
                2,
                Body::Closed {
                    reason: Some("error".to_owned()),
                }
            ))
        );
        assert_eq!(
            harness.mock.executor.calls(),
            [(
                "agent.get.request".to_owned(),
                json!({"agentId": agent_of(RUN)})
            )]
        );
        assert!(harness.mock.observer.observed().is_empty());
    }
}

#[tokio::test(start_paused = true)]
async fn recover_reports_cancelled_when_a_cancel_was_requested_before_the_agent_existed() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness.store.request_cancel(RUN).expect("cancel records");
    harness
        .mock
        .executor
        .respond("agent.get.request", Ok(json!({"agent": null})));

    // Act
    harness.coordinator.recover().await;

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Cancelled);
    assert_eq!((status.reason_code, status.reason_detail), (None, None));
    assert_eq!(harness.record(RUN).status, RunState::Cancelled);
    assert!(matches!(
        harness.log(RUN).last(),
        Some((2, Body::Closed { .. }))
    ));
}

#[tokio::test(start_paused = true)]
async fn recover_retries_agent_get_while_the_catalog_is_busy() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness
        .mock
        .executor
        .respond("agent.get.request", Err(ErrorCode::CatalogBusy));
    harness
        .mock
        .executor
        .respond("agent.get.request", Ok(json!({"agent": null})));

    // Act
    harness.coordinator.recover().await;

    // Assert
    assert_eq!(harness.mock.executor.calls_of("agent.get.request").len(), 2);
    let status = single_status(harness.sent());
    assert_eq!(status.reason_code, Some(ReasonCode::SessionLost));
}

#[tokio::test(start_paused = true)]
async fn recover_completes_a_run_whose_agent_was_archived() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness.mock.executor.respond(
        "agent.get.request",
        Ok(json!({"agent": {"id": agent_of(RUN), "archivedAt": "2026-10-04T00:00:00Z", "labels": {"bonsai.run": RUN}}})),
    );

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Completed);
    assert_eq!(status.execution, Some(claude_execution()));
    assert_eq!((status.reason_code, status.reason_detail), (None, None));
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Completed);
    assert_eq!(record.agent_id, Some(agent_of(RUN)));
    assert_eq!(
        harness.log(RUN).last(),
        Some(&(
            2,
            Body::Closed {
                reason: Some("archived".to_owned()),
            }
        ))
    );
    assert!(harness.mock.observer.observed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn recover_restarts_the_session_of_a_live_agent() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness
        .store
        .set_agent(RUN, &agent_of(RUN))
        .expect("agent records");
    harness
        .store
        .advance(RUN, &update(RunState::Running))
        .expect("state advances");
    harness
        .mock
        .executor
        .respond("agent.get.request", Ok(live_agent(RUN)));
    harness
        .mock
        .executor
        .always("agent.message.send.request", Ok(json!({"accepted": true})));

    // Act
    harness.coordinator.recover().await;
    settle().await;
    let after_recover = harness.statuses();
    harness.frame(send_frame(RUN, INPUT));
    settle().await;

    // Assert
    assert!(
        after_recover.is_empty(),
        "a live session keeps its state: {after_recover:?}"
    );
    assert_eq!(
        harness.mock.executor.calls_of("agent.timeline.get.request"),
        [json!({"agentId": agent_of(RUN), "direction": "tail", "limit": 1})]
    );
    assert_eq!(harness.mock.observer.observed(), [agent_of(RUN)]);
    assert!(
        harness
            .mock
            .executor
            .calls_of("agent.create.request")
            .is_empty()
    );
    let sends = harness.mock.executor.calls_of("agent.message.send.request");
    assert_eq!(sends.len(), 1, "the input reached the recovered session");
    assert_eq!(sends[0]["agentId"], agent_of(RUN));
    assert_eq!(harness.record(RUN).status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn recover_leaves_finished_runs_alone() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    harness
        .store
        .advance(RUN, &update(RunState::Completed))
        .expect("state advances");

    // Act
    harness.coordinator.recover().await;

    // Assert
    assert!(harness.sent().is_empty());
    assert!(harness.mock.executor.calls().is_empty());
}

// ---------------------------------------------------------- connection inputs

#[tokio::test]
async fn rejected_frame_without_a_session_is_quarantined_in_a_new_epoch() {
    // Arrange
    let mut harness = Harness::new();
    seed(&harness.store, RUN, 5);
    let text = |seq: u64| {
        event_json(
            seq,
            Body::Text {
                mid: "m1".to_owned(),
                text: format!("part {seq}"),
            },
        )
    };
    harness
        .store
        .append(RUN, "e1", 0, &[text(0), text(1), text(2)], None)
        .expect("log appends");

    // Act
    harness.coordinator.handle(Input::Rejected {
        run_id: RUN.to_owned(),
        epoch: "e1".to_owned(),
        from: 1,
        to: 1,
    });

    // Assert
    let record = harness.record(RUN);
    assert_ne!(record.epoch, "e1");
    assert_eq!(record.next_seq, 3);
    let log = harness.log(RUN);
    assert!(matches!(
        log[1],
        (
            1,
            Body::Notice {
                level: Level::Error,
                ..
            }
        )
    ));
    assert_eq!(
        log[2].1,
        Body::Text {
            mid: "m1".to_owned(),
            text: "part 2".to_owned(),
        }
    );
    assert!(harness.sent().is_empty());
}

#[tokio::test(start_paused = true)]
async fn revocation_interrupts_and_closes_live_sessions_without_reporting() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    let _ = harness.sent();

    // Act
    harness.coordinator.handle(Input::Revoked);
    settle().await;

    // Assert
    assert!(
        harness.statuses().is_empty(),
        "nothing is reported after a revocation"
    );
    assert_eq!(
        harness.mock.executor.calls_of("agent.cancel.request").len(),
        1
    );
    assert_eq!(
        harness
            .mock
            .executor
            .calls_of("agent.archive.request")
            .len(),
        1
    );
    assert_eq!(
        harness.log(RUN).last().map(|(_, body)| body.clone()),
        Some(Body::Closed {
            reason: Some("revoked".to_owned()),
        })
    );
    assert_eq!(
        harness.notices.try_recv().ok(),
        Some(Notice::Ended(RUN.to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_leaves_live_agents_for_recovery() {
    // Arrange
    let mut harness = Harness::new();
    harness.creates_succeed();
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    let _ = harness.sent();

    // Act
    harness.coordinator.handle(Input::Shutdown);
    settle().await;

    // Assert
    assert!(harness.statuses().is_empty());
    assert!(
        harness
            .mock
            .executor
            .calls_of("agent.cancel.request")
            .is_empty()
    );
    assert!(
        harness
            .mock
            .executor
            .calls_of("agent.archive.request")
            .is_empty()
    );
    assert_eq!(harness.record(RUN).status, RunState::Running);
    assert_eq!(
        harness.notices.try_recv().ok(),
        Some(Notice::Ended(RUN.to_owned()))
    );
}

#[tokio::test(start_paused = true)]
async fn run_serves_inputs_until_shutdown() {
    // Arrange
    let Harness {
        coordinator,
        mut frames,
        notices,
        ..
    } = Harness::new();
    let (inputs, queue) = mpsc::unbounded_channel();
    for input in [
        Input::Connected(member()),
        Input::Frame(Inbound::Query {
            run_id: RUN.to_owned(),
        }),
        Input::Shutdown,
        Input::Frame(Inbound::Query {
            run_id: RUN.to_owned(),
        }),
    ] {
        inputs.send(input).expect("coordinator queue is open");
    }

    // Act
    coordinator.run(queue, notices).await;

    // Assert
    let mut sent = Vec::new();
    while let Ok(outgoing) = frames.try_recv() {
        if !matches!(outgoing, Outgoing::Credit { .. }) {
            sent.push(outgoing);
        }
    }
    let status = single_status(sent);
    assert_eq!(status.reason_code, Some(ReasonCode::RunUnknown));
}

#[tokio::test(start_paused = true)]
async fn run_stops_when_its_input_queue_closes() {
    // Arrange
    let Harness {
        coordinator,
        notices,
        ..
    } = Harness::new();
    let (inputs, queue) = mpsc::unbounded_channel::<Input>();
    drop(inputs);

    // Act and Assert: returns instead of waiting forever.
    tokio::time::timeout(Duration::from_secs(1), coordinator.run(queue, notices))
        .await
        .expect("run returns once its inputs close");
}

// ------------------------------------------------------------- write policy

#[test]
fn write_policy_injects_claude_and_codex_only_for_a_loopback_bonsai() {
    for loopback in [false, true] {
        // Arrange
        let policy = WritePolicy { loopback };

        // Act
        let injected = ["claude", "codex", "gemini"].map(|provider| policy.inject(provider));

        // Assert
        assert_eq!(injected, [loopback, loopback, false], "{policy:?}");
    }
}

#[test]
fn write_policy_reports_bonsai_write_when_the_url_is_injected() {
    for (loopback, claude, codex) in [(false, false, false), (true, true, true)] {
        // Arrange
        let policy = WritePolicy { loopback };

        // Act and Assert
        assert_eq!(
            [
                policy.bonsai_write("claude"),
                policy.bonsai_write("codex"),
                policy.bonsai_write("gemini")
            ],
            [claude, codex, false],
            "{policy:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn recover_reports_running_for_a_claimed_run_whose_agent_was_created() {
    // Arrange: AIT stopped after agent.create succeeded but before `running` was recorded.
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness
        .mock
        .executor
        .respond("agent.get.request", Ok(live_agent(RUN)));

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert
    let record = harness.record(RUN);
    assert_eq!(record.status, RunState::Running);
    assert_eq!(record.agent_id.as_deref(), Some(agent_of(RUN).as_str()));
}

#[tokio::test(start_paused = true)]
async fn recover_never_attaches_an_agent_whose_snapshot_it_cannot_read() {
    // Arrange: a storage error leaves the Agent's label unchecked.
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness
        .store
        .set_agent(RUN, &agent_of(RUN))
        .expect("agent records");
    harness
        .store
        .advance(RUN, &update(RunState::Running))
        .expect("state advances");
    harness
        .mock
        .executor
        .respond("agent.get.request", Err(ErrorCode::AgentIo));

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert: the run ends, and the Agent is neither observed nor touched.
    let status = single_status(harness.sent());
    assert_eq!(status.status, RunState::Failed);
    assert_eq!(status.reason_code, Some(ReasonCode::SessionLost));
    assert_eq!(
        status.reason_detail.as_deref(),
        Some("执行端重启之后读不到这个会话的状态")
    );
    assert!(harness.mock.observer.observed().is_empty());
    assert_eq!(harness.mock.executor.calls().len(), 1, "only the agent.get");
}

#[tokio::test(start_paused = true)]
async fn recover_treats_a_snapshot_without_labels_as_another_agent() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness.mock.executor.respond(
        "agent.get.request",
        Ok(json!({"agent": {"id": agent_of(RUN), "pendingPermissions": []}})),
    );

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert
    assert_eq!(
        single_status(harness.sent()).reason_code,
        Some(ReasonCode::SessionLost)
    );
    assert!(harness.mock.observer.observed().is_empty());
}

#[tokio::test]
async fn queries_and_cancels_credit_their_run_before_the_status() {
    // Arrange
    let mut harness = Harness::new();

    // Act
    harness.frame(Inbound::Query {
        run_id: RUN.to_owned(),
    });
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });
    let mut raw = Vec::new();
    while let Ok(outgoing) = harness.frames.try_recv() {
        raw.push(outgoing);
    }

    // Assert: each credit precedes the status it pays for.
    let credit = Outgoing::Credit {
        run_id: RUN.to_owned(),
    };
    assert_eq!(raw.len(), 4, "{raw:?}");
    assert_eq!(raw[0], credit);
    assert!(matches!(raw[1], Outgoing::Status(_)));
    assert_eq!(raw[2], credit);
    assert!(matches!(raw[3], Outgoing::Status(_)));
}

#[tokio::test(start_paused = true)]
async fn recover_never_attaches_an_agent_without_the_runs_label() {
    // Arrange: the derived ID names an Agent that is not this run's.
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    harness.mock.executor.respond(
        "agent.get.request",
        Ok(json!({"agent": {"id": agent_of(RUN), "labels": {"bonsai.run": "r_ffffffffffffffffffffffffffffffff"}}})),
    );

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert: reported lost, and the foreign Agent is never touched again.
    let status = single_status(harness.sent());
    assert_eq!(status.reason_code, Some(ReasonCode::SessionLost));
    assert_eq!(harness.mock.executor.calls().len(), 1, "only the agent.get");
}

#[tokio::test(start_paused = true)]
async fn closing_a_lost_runs_log_settles_its_requests_and_rejects_its_queue() {
    // Arrange
    let mut harness = Harness::new();
    seed_open(&harness.store, RUN);
    for (ask_id, state, by, effect) in [
        ("ask-pending", "pending", None, None),
        (
            "ask-resolving",
            "resolving",
            Some(serde_json::to_string(&member()).expect("json")),
            Some("allow"),
        ),
        ("ask-done", "resolved", None, None),
    ] {
        harness
            .store
            .put_ask(
                RUN,
                &crate::store::AskRecord {
                    ask_id: ask_id.to_owned(),
                    spec: "{}".to_owned(),
                    state: state.to_owned(),
                    resolving_by: by,
                    resolving_effect: effect.map(str::to_owned),
                },
            )
            .expect("store the request");
    }
    harness
        .store
        .insert_input(
            RUN,
            &crate::store::InputRecord {
                input_id: "in-q".to_owned(),
                message_id: uuid_of("in-q"),
                text: "waiting".to_owned(),
                state: "queued".to_owned(),
                by: Some(member().id),
                login: None,
            },
        )
        .expect("store the input");
    harness
        .mock
        .executor
        .respond("agent.get.request", Ok(json!({"agent": null})));

    // Act
    harness.coordinator.recover().await;
    settle().await;

    // Assert
    let tail: Vec<Body> = harness
        .log(RUN)
        .into_iter()
        .skip(2)
        .map(|(_, body)| body)
        .collect();
    assert_eq!(tail.len(), 4, "{tail:?}");
    assert!(tail.contains(&Body::AskResolved {
        id: "ask-pending".to_owned(),
        outcome: crate::event::Outcome::Withdrawn,
        by: None,
        login: None,
    }));
    assert!(tail.contains(&Body::AskResolved {
        id: "ask-resolving".to_owned(),
        outcome: crate::event::Outcome::Allow,
        by: Some(member().id),
        login: member().login,
    }));
    assert_eq!(
        tail[2],
        Body::InputRejected {
            id: "in-q".to_owned(),
            code: RejectCode::Closed,
            reason: None,
        }
    );
    assert_eq!(
        tail[3],
        Body::Closed {
            reason: Some("error".to_owned())
        }
    );
    let states: Vec<String> = harness
        .store
        .asks(RUN)
        .expect("asks")
        .into_iter()
        .map(|ask| ask.state)
        .collect();
    assert!(states.iter().all(|state| state == "resolved"), "{states:?}");
}

#[tokio::test]
async fn the_machine_owner_survives_a_restart() {
    // Arrange
    let mut first = Harness::new();
    first.coordinator.handle(Input::Connected(member()));

    // Act: a new coordinator on the same store, before any connection.
    let (notices, _) = mpsc::unbounded_channel();
    let second = Coordinator::new(
        first.mock.host(),
        first.store.clone(),
        Outbox::default(),
        Arc::new(RwLock::new(Offer::default())),
        notices,
        LOCAL,
    );

    // Assert
    assert_eq!(second.owner, Some(member()));
    let _ = first.sent();
}

#[tokio::test(start_paused = true)]
async fn commands_for_a_session_that_stopped_taking_them_wait_until_it_ends() {
    // Arrange: a session that finished its turn, then was cancelled and has stopped.
    let mut harness = Harness::new();
    harness.creates_succeed();
    harness
        .mock
        .executor
        .always("agent.archive.request", Ok(json!({})));
    harness.dispatch(dispatch_for(RUN));
    settle().await;
    harness.mock.observer.emit(
        &agent_of(RUN),
        "agent_stream",
        json!({"event": {"type": "turn_completed"}}),
    );
    settle().await;
    harness.frame(Inbound::Cancel {
        run_id: RUN.to_owned(),
    });
    settle().await;
    let before = harness.log(RUN);

    // Act: input reaches the coordinator before it learned the session ended.
    harness.frame(Inbound::Send {
        run_id: RUN.to_owned(),
        by: member(),
        input_id: INPUT.to_owned(),
        text: "too late".to_owned(),
    });
    let while_closing = harness.log(RUN);
    harness.coordinator.ended(RUN);
    let after = harness.log(RUN);

    // Assert: nothing is written until the session is gone, then one rejection follows.
    assert_eq!(
        while_closing, before,
        "the session's log has a single writer"
    );
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(
        after.last().map(|(_, body)| body.clone()),
        Some(Body::InputRejected {
            id: INPUT.to_owned(),
            code: RejectCode::Closed,
            reason: None,
        })
    );
    for (index, (seq, _)) in after.iter().enumerate() {
        assert_eq!(*seq, index as u64, "contiguous seqs");
    }
}
