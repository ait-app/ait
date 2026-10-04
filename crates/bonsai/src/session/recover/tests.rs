use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{LogState, fold, spawn};
use crate::event::{AskKind, AskOption, Body, Effect, Event, Level, Origin, Outcome, TurnState};
use crate::outbox::{Bucket, Outbox, Outgoing};
use crate::ports::{Backlog, Row};
use crate::session::{Command, Context, Notice, SUBAGENT_NOTICE, Start};
use crate::settings::Applied;
use crate::store::{AskRecord, InputRecord, StatusUpdate, Store};
use crate::testing::{MockHost, dispatch};
use crate::translate::{AskSpec, uuid_of};
use crate::wire::{Answer, Execution, Person, RunState};

const RUN: &str = "r_0123456789abcdef0123456789abcdef";
const EPOCH: &str = "e-0123456789ab";
const AIT_EPOCH: &str = "ait-1";

fn event(seq: u64, body: Body) -> String {
    Event { seq, at: 1, body }
        .to_json()
        .expect("events serialize")
}

fn serialized(bodies: Vec<Body>) -> Vec<String> {
    bodies
        .into_iter()
        .zip(0..)
        .map(|(body, seq)| event(seq, body))
        .collect()
}

fn turn(state: TurnState) -> Body {
    Body::Turn {
        state,
        reason: None,
    }
}

fn text(mid: &str, text: &str) -> Body {
    Body::Text {
        mid: mid.to_owned(),
        text: text.to_owned(),
    }
}

fn notice(text: &str) -> Body {
    Body::Notice {
        level: Level::Info,
        text: text.to_owned(),
    }
}

fn dispatch_input() -> Body {
    Body::Input {
        id: "dispatch-01234567".to_owned(),
        text: "- [ ] list the files".to_owned(),
        by: "github:900002".to_owned(),
        login: Some("sandbox-member".to_owned()),
        origin: Origin::Dispatch,
    }
}

fn user_input(id: &str, text: &str) -> Body {
    Body::Input {
        id: id.to_owned(),
        text: text.to_owned(),
        by: "github:900002".to_owned(),
        login: Some("sandbox-member".to_owned()),
        origin: Origin::User,
    }
}

fn aborted_by_restart() -> Body {
    Body::Turn {
        state: TurnState::Aborted,
        reason: Some("runtime_restarted".to_owned()),
    }
}

fn withdrawn(id: &str) -> Body {
    Body::AskResolved {
        id: id.to_owned(),
        outcome: Outcome::Withdrawn,
        by: None,
        login: None,
    }
}

fn agent_id() -> String {
    uuid_of(RUN.trim_start_matches("r_"))
}

fn execution() -> Execution {
    Execution {
        provider: "claude".to_owned(),
        model: None,
        approvals: true,
        bonsai_write: false,
    }
}

fn member() -> Person {
    Person {
        id: "github:900002".to_owned(),
        login: Some("sandbox-member".to_owned()),
    }
}

/// A Claude tool request offering a one-time allow and a deny.
fn spec(id: &str) -> AskSpec {
    AskSpec {
        id: id.to_owned(),
        native_id: None,
        provider: "claude".to_owned(),
        kind: AskKind::Tool,
        title: "Bash: ls".to_owned(),
        detail: Some("ls".to_owned()),
        truncated: false,
        options: vec![
            AskOption {
                id: "allow".to_owned(),
                label: "允许这一次".to_owned(),
                effect: Effect::Allow,
            },
            AskOption {
                id: "deny".to_owned(),
                label: "拒绝".to_owned(),
                effect: Effect::Deny,
            },
        ],
        questions: None,
        answer_keys: Vec::new(),
        native_actions: vec!["allow".to_owned(), "deny".to_owned()],
    }
}

fn pending_ask(id: &str) -> AskRecord {
    AskRecord {
        ask_id: id.to_owned(),
        spec: serde_json::to_string(&spec(id)).expect("specs serialize"),
        state: "pending".to_owned(),
        resolving_by: None,
        resolving_effect: None,
    }
}

fn resolving_ask(id: &str, by: &Person, effect: &str) -> AskRecord {
    AskRecord {
        state: "resolving".to_owned(),
        resolving_by: Some(serde_json::to_string(by).expect("people serialize")),
        resolving_effect: Some(effect.to_owned()),
        ..pending_ask(id)
    }
}

fn input(id: &str, text: &str, state: &str) -> InputRecord {
    InputRecord {
        input_id: id.to_owned(),
        message_id: uuid_of(id),
        text: text.to_owned(),
        state: state.to_owned(),
        by: Some("github:900002".to_owned()),
        login: Some("sandbox-member".to_owned()),
    }
}

/// `agent.get` of a live, unarchived Agent still holding `pending` requests.
fn snapshot(pending: &[&str]) -> Value {
    let requests: Vec<Value> = pending.iter().map(|id| json!({ "id": id })).collect();
    json!({"agent": {"id": agent_id(), "status": "idle", "pendingPermissions": requests}})
}

fn row(seq: u64, item: Value) -> Row {
    Row {
        seq,
        provider: "claude".to_owned(),
        turn_id: None,
        item,
    }
}

fn said(mid: &str, text: &str) -> Value {
    json!({"type": "assistant_message", "messageId": mid, "text": text})
}

/// Yield to the session task until `condition` holds.
async fn until(condition: impl Fn() -> bool) {
    for _ in 0..1000 {
        if condition() {
            return;
        }
        tokio::task::yield_now().await;
    }
    assert!(condition(), "the session never reached the expected point");
}

/// A recovered session's surroundings: mocked host ports, an in-memory store, an attached
/// outbox and the coordinator's notice channel.
struct Harness {
    host: MockHost,
    store: Store,
    context: Context,
    outgoing: mpsc::UnboundedReceiver<Outgoing>,
    notices: mpsc::UnboundedReceiver<Notice>,
}

impl Harness {
    fn new() -> Self {
        let host = MockHost::new();
        let store = Store::memory().expect("in-memory store");
        let outbox = Outbox::default();
        let (sender, outgoing) = mpsc::unbounded_channel();
        outbox.attach(sender);
        let (notice_sender, notices) = mpsc::unbounded_channel();
        let context = Context {
            host: host.host(),
            store: store.clone(),
            outbox,
            notices: notice_sender,
            polls: Arc::new(Mutex::new(Bucket::new(1.0, 1.0))),
        };
        Self {
            host,
            store,
            context,
            outgoing,
            notices,
        }
    }

    /// Record a running run whose log holds `bodies`, as the previous process left it.
    fn seed(&self, bodies: Vec<Body>, ait_cursor: Option<(&str, u64)>) {
        self.store
            .insert_run(&dispatch(RUN), EPOCH, 1)
            .expect("run inserted");
        self.store
            .set_execution(RUN, &execution())
            .expect("execution recorded");
        self.store
            .set_agent(RUN, &agent_id())
            .expect("agent recorded");
        let running = StatusUpdate {
            status: RunState::Running,
            reason_code: None,
            reason_detail: None,
            final_text: None,
            at: 2,
        };
        assert!(self.store.advance(RUN, &running).expect("status advanced"));
        self.store
            .append(RUN, EPOCH, 0, &serialized(bodies), ait_cursor)
            .expect("log appended");
    }

    fn start(&self) -> Start {
        Start {
            run: self
                .store
                .run(RUN)
                .expect("run readable")
                .expect("run present"),
            execution: execution(),
            project: String::new(),
            applied: Applied {
                mode: "default".to_owned(),
                ..Applied::default()
            },
            owner: None,
            inject: None,
        }
    }

    fn recover(&self, snapshot: Value) -> mpsc::UnboundedSender<Command> {
        spawn(self.context.clone(), self.start(), snapshot)
    }

    /// Recover, then stop the server as soon as the session starts serving.
    async fn recover_and_shut_down(&mut self, snapshot: Value) {
        let commands = self.recover(snapshot);
        commands
            .send(Command::Shutdown)
            .expect("the session takes commands");
        self.ended().await;
    }

    async fn ended(&mut self) {
        let notice = tokio::time::timeout(Duration::from_mins(1), self.notices.recv())
            .await
            .expect("the session ends");
        assert_eq!(notice, Some(Notice::Ended(RUN.to_owned())));
    }

    /// Bodies of the run's current generation from `from` on.
    fn logged_from(&self, from: u64) -> Vec<Body> {
        let run = self
            .store
            .run(RUN)
            .expect("run readable")
            .expect("run present");
        self.store
            .events(RUN, &run.epoch, from, run.next_seq)
            .expect("log readable")
            .iter()
            .map(|json| {
                serde_json::from_str::<Event>(json)
                    .expect("logged events parse")
                    .body
            })
            .collect()
    }

    fn sent(&mut self) -> Vec<Outgoing> {
        let mut sent = Vec::new();
        while let Ok(outgoing) = self.outgoing.try_recv() {
            sent.push(outgoing);
        }
        sent
    }

    fn ask_state(&self, id: &str) -> String {
        self.store
            .asks(RUN)
            .expect("asks readable")
            .into_iter()
            .find(|ask| ask.ask_id == id)
            .map(|ask| ask.state)
            .expect("ask recorded")
    }

    fn input_state(&self, id: &str) -> String {
        self.store
            .inputs(RUN)
            .expect("inputs readable")
            .into_iter()
            .find(|input| input.input_id == id)
            .map(|input| input.state)
            .expect("input recorded")
    }
}

#[test]
fn fold_of_an_empty_log_is_the_default_state() {
    // Arrange
    let events: Vec<String> = Vec::new();

    // Act
    let state = fold(&events);

    // Assert
    assert_eq!(state, LogState::default());
}

#[test]
fn fold_reports_a_turn_that_started_and_never_ended_as_open() {
    // Arrange
    let events = serialized(vec![
        dispatch_input(),
        turn(TurnState::Started),
        text("m-1", "working"),
    ]);

    // Act
    let state = fold(&events);

    // Assert
    assert!(state.turn_open);
    assert!(!state.last_turn_failed);
    assert!(state.closed.is_none());
}

#[test]
fn fold_closes_the_turn_on_every_ending_and_remembers_only_failure() {
    let cases = [
        (TurnState::Completed, false),
        (TurnState::Failed, true),
        (TurnState::Aborted, false),
    ];
    for (ending, failed) in cases {
        // Arrange
        let events = serialized(vec![turn(TurnState::Started), turn(ending)]);

        // Act
        let state = fold(&events);

        // Assert
        assert!(!state.turn_open, "{ending:?} ends the turn");
        assert_eq!(state.last_turn_failed, failed, "{ending:?}");
    }
}

#[test]
fn fold_keeps_the_last_failure_until_a_later_turn_ends() {
    // Arrange
    let failed_then_restarted = serialized(vec![
        turn(TurnState::Started),
        turn(TurnState::Failed),
        turn(TurnState::Started),
    ]);
    let failed_then_completed = serialized(vec![
        turn(TurnState::Started),
        turn(TurnState::Failed),
        turn(TurnState::Started),
        turn(TurnState::Completed),
    ]);

    // Act
    let running = fold(&failed_then_restarted);
    let ended = fold(&failed_then_completed);

    // Assert
    assert!(running.turn_open);
    assert!(running.last_turn_failed);
    assert!(!ended.turn_open);
    assert!(!ended.last_turn_failed);
}

#[test]
fn fold_marks_the_session_closed_and_ignores_turns_after_it() {
    // Arrange
    let events = serialized(vec![
        turn(TurnState::Started),
        Body::Closed {
            reason: Some("idle".to_owned()),
        },
        turn(TurnState::Started),
    ]);

    // Act
    let state = fold(&events);

    // Assert
    assert!(state.closed.is_some());
    assert!(!state.turn_open);
}

#[test]
fn fold_concatenates_the_deltas_of_the_last_assistant_message() {
    // Arrange
    let events = serialized(vec![
        text("m-1", "first message"),
        text("m-2", "Hel"),
        Body::Reasoning {
            mid: None,
            text: "thinking".to_owned(),
        },
        text("m-2", "lo"),
    ]);

    // Act
    let state = fold(&events);

    // Assert
    assert_eq!(state.messages.final_text().as_deref(), Some("Hello"));
}

#[test]
fn fold_starts_over_when_a_new_message_begins() {
    // Arrange
    let events = serialized(vec![
        text("m-1", "Hel"),
        text("m-1", "lo"),
        text("m-2", "Bye"),
    ]);

    // Act
    let state = fold(&events);

    // Assert
    assert_eq!(state.messages.final_text().as_deref(), Some("Bye"));
}

#[test]
fn fold_appends_interleaved_text_to_its_own_message() {
    // Arrange: protocol §6 — the same mid continues its message across other entries.
    let events = serialized(vec![
        text("m-a", "a1"),
        text("m-b", "b1"),
        text("m-a", "a2"),
    ]);

    // Act
    let state = fold(&events);

    // Assert
    assert_eq!(state.messages.final_text().as_deref(), Some("a1a2"));
}

#[test]
fn fold_notes_only_the_subagent_notice() {
    // Arrange
    let other_notices = serialized(vec![notice("上下文已压缩(Context compacted)")]);
    let subagent = serialized(vec![notice("before"), notice(SUBAGENT_NOTICE)]);

    // Act
    let without = fold(&other_notices);
    let with = fold(&subagent);

    // Assert
    assert!(!without.subagent_noted);
    assert!(with.subagent_noted);
}

#[test]
fn fold_skips_unreadable_events_without_disturbing_the_rest() {
    // Arrange
    let readable = vec![
        event(0, turn(TurnState::Started)),
        event(1, text("m-1", "Hel")),
        event(2, text("m-1", "lo")),
    ];
    let mixed = vec![
        "not json".to_owned(),
        event(0, turn(TurnState::Started)),
        r#"{"seq":1,"at":1,"t":"teleport"}"#.to_owned(),
        event(1, text("m-1", "Hel")),
        r#"{"seq":2,"at":1,"t":"turn","state":"paused"}"#.to_owned(),
        r#"{"seq":3,"at":1,"t":"text","mid":"m-1"}"#.to_owned(),
        r#"{"seq":4,"at":1,"t":"closed","reason":7}"#.to_owned(),
        "{}".to_owned(),
        event(2, text("m-1", "lo")),
    ];

    // Act
    let state = fold(&mixed);

    // Assert
    assert_eq!(state, fold(&readable));
    assert!(state.turn_open);
    assert!(state.closed.is_none());
    assert_eq!(state.messages.final_text().as_deref(), Some("Hello"));
}

#[tokio::test(start_paused = true)]
async fn an_interrupted_turn_withdraws_open_asks_and_is_aborted_by_the_restart() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            spec("perm-1").body(),
            spec("perm-2").body(),
        ],
        None,
    );
    harness
        .store
        .put_ask(RUN, &pending_ask("perm-1"))
        .expect("ask recorded");
    harness
        .store
        .put_ask(RUN, &pending_ask("perm-2"))
        .expect("ask recorded");

    // Act
    harness.recover_and_shut_down(snapshot(&["perm-2"])).await;

    // Assert
    assert_eq!(
        harness.logged_from(4),
        vec![
            withdrawn("perm-1"),
            withdrawn("perm-2"),
            aborted_by_restart()
        ]
    );
    assert_eq!(harness.ask_state("perm-1"), "resolved");
    assert_eq!(harness.ask_state("perm-2"), "resolved");
    let run = harness
        .store
        .run(RUN)
        .expect("run readable")
        .expect("run present");
    assert_eq!(run.status, RunState::Running);
    assert!(
        harness
            .host
            .executor
            .calls_of("agent.message.send.request")
            .is_empty()
    );
}

#[tokio::test(start_paused = true)]
async fn a_resolving_answer_the_agent_dropped_is_settled_with_the_stored_effect_and_member() {
    // Arrange
    let mut harness = Harness::new();
    let owner = Person {
        id: "github:1".to_owned(),
        login: Some("machine-owner".to_owned()),
    };
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            spec("perm-1").body(),
            spec("perm-2").body(),
        ],
        None,
    );
    harness
        .store
        .put_ask(RUN, &resolving_ask("perm-1", &member(), "allow"))
        .expect("ask recorded");
    harness
        .store
        .put_ask(RUN, &resolving_ask("perm-2", &owner, "deny"))
        .expect("ask recorded");

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert
    let logged = harness.logged_from(4);
    assert_eq!(logged.len(), 3, "{logged:?}");
    assert!(logged[..2].contains(&Body::AskResolved {
        id: "perm-1".to_owned(),
        outcome: Outcome::Allow,
        by: Some("github:900002".to_owned()),
        login: Some("sandbox-member".to_owned()),
    }));
    assert!(logged[..2].contains(&Body::AskResolved {
        id: "perm-2".to_owned(),
        outcome: Outcome::Deny,
        by: Some("github:1".to_owned()),
        login: Some("machine-owner".to_owned()),
    }));
    assert_eq!(logged[2], aborted_by_restart());
    assert_eq!(harness.ask_state("perm-1"), "resolved");
    assert_eq!(harness.ask_state("perm-2"), "resolved");
}

#[tokio::test(start_paused = true)]
async fn requests_the_agent_still_holds_stay_answerable_after_recovery() {
    // Arrange: the turn has ended in the log, but the provider kept its requests (codex can).
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            spec("perm-1").body(),
            spec("perm-2").body(),
            turn(TurnState::Completed),
        ],
        None,
    );
    harness
        .store
        .put_ask(RUN, &pending_ask("perm-1"))
        .expect("ask recorded");
    harness
        .store
        .put_ask(RUN, &resolving_ask("perm-2", &member(), "allow"))
        .expect("ask recorded");
    harness
        .host
        .executor
        .always("agent.permission.resolve.request", Ok(json!({})));
    let answer = |ask_id: &str, option_id: &str| {
        Command::Answer(Box::new(Answer {
            run_id: RUN.to_owned(),
            by: member(),
            ask_id: ask_id.to_owned(),
            option_id: option_id.to_owned(),
            answers: None,
            note: None,
        }))
    };

    // Act
    let commands = harness.recover(snapshot(&["perm-1", "perm-2"]));
    for command in [
        answer("perm-1", "allow"),
        answer("perm-2", "deny"),
        Command::Shutdown,
    ] {
        commands.send(command).expect("the session takes commands");
    }
    harness.ended().await;

    // Assert
    assert!(harness.logged_from(5).is_empty());
    assert_eq!(
        harness
            .host
            .executor
            .calls_of("agent.permission.resolve.request"),
        vec![
            json!({"agentId": agent_id(), "requestId": "perm-1",
                   "response": {"behavior": "allow", "selectedActionId": "allow"}}),
            json!({"agentId": agent_id(), "requestId": "perm-2",
                   "response": {"behavior": "deny", "selectedActionId": "deny", "interrupt": false}}),
        ]
    );
    assert_eq!(harness.ask_state("perm-1"), "resolving");
    assert_eq!(harness.ask_state("perm-2"), "resolving");
}

#[tokio::test(start_paused = true)]
async fn queued_inputs_are_delivered_one_at_a_time_after_recovery() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            text("m-1", "done"),
            turn(TurnState::Completed),
            user_input("in-0", "earlier"),
            turn(TurnState::Started),
            turn(TurnState::Completed),
            user_input("in-1", "first"),
            user_input("in-2", "second"),
        ],
        None,
    );
    for record in [
        input("in-0", "earlier", "sent"),
        input("in-1", "first", "queued"),
        input("in-2", "second", "queued"),
    ] {
        assert!(
            harness
                .store
                .insert_input(RUN, &record)
                .expect("input recorded")
        );
    }
    harness
        .host
        .executor
        .always("agent.message.send.request", Ok(json!({"accepted": true})));

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert
    assert_eq!(
        harness.host.executor.calls_of("agent.message.send.request"),
        vec![
            json!({"agentId": agent_id(), "text": "first", "messageId": uuid_of("in-1"),
                    "activeTurnBehavior": "steer"})
        ]
    );
    assert_eq!(harness.logged_from(9), vec![turn(TurnState::Started)]);
    assert_eq!(harness.input_state("in-0"), "sent");
    assert_eq!(harness.input_state("in-1"), "sent");
    assert_eq!(harness.input_state("in-2"), "queued");
}

#[tokio::test(start_paused = true)]
async fn an_interrupted_turn_is_aborted_before_the_next_queued_input_starts() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            user_input("in-1", "follow-up"),
        ],
        None,
    );
    assert!(
        harness
            .store
            .insert_input(RUN, &input("in-1", "follow-up", "queued"))
            .expect("input recorded")
    );
    harness
        .host
        .executor
        .always("agent.message.send.request", Ok(json!({"accepted": true})));

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert
    assert_eq!(
        harness.logged_from(3),
        vec![aborted_by_restart(), turn(TurnState::Started)]
    );
    let sends = harness.host.executor.calls_of("agent.message.send.request");
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0]["text"], "follow-up");
    assert_eq!(harness.input_state("in-1"), "sent");
}

#[tokio::test(start_paused = true)]
async fn recovery_reconciles_the_native_history_before_backfilling() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            turn(TurnState::Completed),
        ],
        Some((AIT_EPOCH, 1)),
    );
    harness.host.backfill.set(
        &agent_id(),
        Backlog {
            epoch: AIT_EPOCH.to_owned(),
            rows: vec![row(2, said("m-2", "after the restart"))],
        },
    );
    let gate = harness.host.executor.hold("agent.timeline.get.request");
    let executor = Arc::clone(&harness.host.executor);

    // Act
    let commands = harness.recover(snapshot(&[]));
    until(|| !executor.calls_of("agent.timeline.get.request").is_empty()).await;
    let while_reconciling = harness.logged_from(3);
    gate.add_permits(1);
    commands
        .send(Command::Shutdown)
        .expect("the session takes commands");
    harness.ended().await;

    // Assert
    assert_eq!(harness.host.observer.observed(), vec![agent_id()]);
    assert_eq!(
        executor.calls_of("agent.timeline.get.request"),
        vec![json!({"agentId": agent_id(), "direction": "tail", "limit": 1})]
    );
    assert!(while_reconciling.is_empty(), "{while_reconciling:?}");
    assert_eq!(
        harness.logged_from(3),
        vec![text("m-2", "after the restart")]
    );
}

#[tokio::test(start_paused = true)]
async fn backfill_translates_only_rows_after_the_stored_ait_cursor() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            text("m-5", "seen"),
            turn(TurnState::Completed),
        ],
        Some((AIT_EPOCH, 5)),
    );
    harness.host.backfill.set(
        &agent_id(),
        Backlog {
            epoch: AIT_EPOCH.to_owned(),
            rows: vec![
                row(4, said("m-4", "seen before")),
                row(5, said("m-5", "seen")),
                row(6, said("m-6", "missed")),
                row(7, said("m-7", "also missed")),
            ],
        },
    );

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert
    assert_eq!(
        harness.logged_from(4),
        vec![text("m-6", "missed"), text("m-7", "also missed")]
    );
    let run = harness
        .store
        .run(RUN)
        .expect("run readable")
        .expect("run present");
    assert_eq!(run.epoch, EPOCH);
    assert_eq!(run.ait_cursor, Some((AIT_EPOCH.to_owned(), 7)));
}

#[tokio::test(start_paused = true)]
async fn backfill_drops_echoes_of_inputs_this_adapter_sent() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            turn(TurnState::Completed),
            user_input("in-1", "again"),
        ],
        Some((AIT_EPOCH, 0)),
    );
    assert!(
        harness
            .store
            .insert_input(RUN, &input("in-1", "again", "sent"))
            .expect("input recorded")
    );
    let dispatch_echo = json!({"type": "user_message", "messageId": "native-1",
        "clientMessageId": uuid_of(&format!("{RUN}:dispatch")), "text": "the dispatch prompt"});
    let input_echo = json!({"type": "user_message", "messageId": uuid_of("in-1"), "text": "again"});
    let owner_message =
        json!({"type": "user_message", "messageId": "owner-1", "text": "from the owner"});
    harness.host.backfill.set(
        &agent_id(),
        Backlog {
            epoch: AIT_EPOCH.to_owned(),
            rows: vec![
                row(1, dispatch_echo),
                row(2, input_echo),
                row(3, owner_message),
            ],
        },
    );

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert
    assert_eq!(
        harness.logged_from(4),
        vec![Body::Input {
            id: "owner-1".to_owned(),
            text: "from the owner".to_owned(),
            by: "runtime:local".to_owned(),
            login: None,
            origin: Origin::User,
        }]
    );
}

#[tokio::test(start_paused = true)]
async fn a_new_ait_generation_after_a_restart_keeps_the_log_and_moves_the_cursor() {
    // Arrange: AIT reconciled native history on reload and started a new generation.
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            text("m-1", "before"),
            turn(TurnState::Completed),
        ],
        Some((AIT_EPOCH, 3)),
    );
    harness.host.backfill.set(
        &agent_id(),
        Backlog {
            epoch: "ait-2".to_owned(),
            rows: vec![
                row(0, said("m-a", "rebuilt")),
                row(1, said("m-b", "history")),
            ],
        },
    );

    // Act
    harness.recover_and_shut_down(snapshot(&[])).await;

    // Assert: the log already sent stays valid; only the AIT cursor moves.
    let run = harness
        .store
        .run(RUN)
        .expect("run readable")
        .expect("run present");
    assert_eq!(run.epoch, EPOCH);
    assert_eq!(run.next_seq, 4);
    assert_eq!(run.ait_cursor, Some(("ait-2".to_owned(), 1)));
    assert_eq!(
        harness
            .store
            .events(RUN, EPOCH, 0, 4)
            .expect("log intact")
            .len(),
        4
    );
    assert!(
        !harness
            .sent()
            .iter()
            .any(|outgoing| matches!(outgoing, Outgoing::Epoch { .. })),
        "no rotation"
    );
}

#[tokio::test(start_paused = true)]
async fn a_log_that_already_closed_only_reports_the_final_state() {
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            text("m-1", "Hel"),
            text("m-1", "lo"),
            turn(TurnState::Completed),
            Body::Closed {
                reason: Some("idle".to_owned()),
            },
        ],
        None,
    );

    // Act
    let _commands = harness.recover(snapshot(&[]));
    harness.ended().await;

    // Assert
    let run = harness
        .store
        .run(RUN)
        .expect("run readable")
        .expect("run present");
    assert_eq!(run.status, RunState::Completed);
    assert_eq!(run.final_text.as_deref(), Some("Hello"));
    assert_eq!(run.next_seq, 6);
    match harness.sent().as_slice() {
        [Outgoing::Status(status)] => {
            assert_eq!(status.run_id, RUN);
            assert_eq!(status.status, RunState::Completed);
            assert_eq!(status.final_text.as_deref(), Some("Hello"));
            assert_eq!(status.execution, Some(execution()));
        }
        other => panic!("expected one status frame, got {other:?}"),
    }
    assert!(harness.host.executor.calls().is_empty());
    assert!(harness.host.observer.observed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_cancel_recorded_before_the_restart_ends_the_run_cancelled() {
    // A recorded cancel survives only when the run was busy (an idle cancel finishes at once),
    // and the restart already stopped that turn: the honest answer is cancelled.
    // Arrange
    let mut harness = Harness::new();
    harness.seed(
        vec![
            dispatch_input(),
            turn(TurnState::Started),
            text("m-1", "done"),
            turn(TurnState::Completed),
            user_input("in-1", "one more thing"),
        ],
        None,
    );
    assert!(
        harness
            .store
            .insert_input(RUN, &input("in-1", "one more thing", "queued"))
            .expect("input recorded")
    );
    harness.store.request_cancel(RUN).expect("cancel recorded");
    harness
        .host
        .executor
        .always("agent.archive.request", Ok(json!({})));

    // Act
    let _commands = harness.recover(snapshot(&[]));
    harness.ended().await;

    // Assert
    assert!(
        harness
            .host
            .executor
            .calls_of("agent.message.send.request")
            .is_empty()
    );
    assert_eq!(
        harness.host.executor.calls_of("agent.archive.request"),
        vec![json!({"agentId": agent_id()})]
    );
    assert_eq!(
        harness.logged_from(5),
        vec![
            Body::InputRejected {
                id: "in-1".to_owned(),
                code: crate::event::RejectCode::Closed,
                reason: None,
            },
            Body::Closed {
                reason: Some("cancelled".to_owned()),
            },
        ]
    );
    assert_eq!(harness.input_state("in-1"), "rejected");
    let run = harness
        .store
        .run(RUN)
        .expect("run readable")
        .expect("run present");
    assert_eq!(run.status, RunState::Cancelled);
    assert_eq!(run.final_text.as_deref(), Some("done"));
}
