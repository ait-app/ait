use std::sync::{Arc, Mutex};
use std::time::Duration;

use model::ErrorCode;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{
    CANCEL_GRACE, COALESCE, Command, Context, IDLE_GRACE, Notice, POLL_INTERVAL, QUEUE_LIMIT,
    SUBAGENT_NOTICE, Start, spawn,
};
use crate::event::{Body, Event, Level, Origin, Outcome, RejectCode, TurnState};
use crate::outbox::{Bucket, Outbox, Outgoing};
use crate::ports::{Backlog, Row};
use crate::prompt::PREAMBLE;
use crate::settings::{Applied, BONSAI_SERVER, bonsai_tool_policy, default_mode};
use crate::store::{RunRecord, Store};
use crate::testing::{MockHost, dispatch};
use crate::translate::{Translator, uuid_of};
use crate::wire::{Answer, Execution, Person, ReasonCode, RunState, Status};

const RUN: &str = "r_0123456789abcdef0123456789abcdef";
const AGENT: &str = "01234567-89ab-cdef-0123-456789abcdef";
const PROJECT: &str = "prj_0123456789abcdef";
const MCP_URL: &str = "http://localhost:8860/mcp";
const AIT_EPOCH: &str = "ait-1";

fn owner() -> Person {
    Person {
        id: "github:900001".to_owned(),
        login: Some("machine-owner".to_owned()),
    }
}

fn member() -> Person {
    Person {
        id: "github:900003".to_owned(),
        login: Some("helper".to_owned()),
    }
}

fn execution(provider: &str) -> Execution {
    Execution {
        provider: provider.to_owned(),
        model: None,
        approvals: true,
        bonsai_write: false,
    }
}

// A host whose Agent RPCs succeed unless a test queues another answer.
fn live_host() -> MockHost {
    let host = MockHost::new();
    host.executor.always(
        "agent.create.request",
        Ok(json!({"agentId": AGENT, "agent": {"id": AGENT}})),
    );
    host.executor
        .always("agent.message.send.request", Ok(json!({"accepted": true})));
    host.executor.always("agent.cancel.request", Ok(json!({})));
    host.executor.always("agent.archive.request", Ok(json!({})));
    host.executor
        .always("agent.permission.resolve.request", Ok(json!({})));
    host
}

// A claimed run stored the way the coordinator stores it, and the start it hands the session.
fn start_for(store: &Store, provider: &str) -> Start {
    store
        .insert_run(&dispatch(RUN), "e-000000000001", 0)
        .expect("insert the run");
    let execution = execution(provider);
    store
        .set_execution(RUN, &execution)
        .expect("record the execution");
    let run = store
        .run(RUN)
        .expect("read the run")
        .expect("the run exists");
    Start {
        run,
        execution,
        project: PROJECT.to_owned(),
        applied: Applied {
            mode: default_mode(provider).to_owned(),
            approvals: true,
            ..Applied::default()
        },
        owner: Some(owner()),
        inject: None,
    }
}

async fn settle() {
    for _ in 0..64 {
        tokio::task::yield_now().await;
    }
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..4096 {
        if check() {
            return;
        }
        tokio::task::yield_now().await;
    }
    assert!(check(), "timed out waiting for {what}");
}

// Let the 150 ms coalescing window elapse so pending events are numbered and logged.
async fn pass_window() {
    tokio::time::sleep(COALESCE + Duration::from_millis(10)).await;
    settle().await;
}

fn assistant(mid: &str, text: &str) -> Value {
    json!({"type": "assistant_message", "messageId": mid, "text": text})
}

struct Harness {
    host: MockHost,
    store: Store,
    frames: mpsc::UnboundedReceiver<Outgoing>,
    sent: Vec<Outgoing>,
    ended: mpsc::UnboundedReceiver<Notice>,
    commands: mpsc::UnboundedSender<Command>,
    polls: Arc<Mutex<Bucket>>,
}

impl Harness {
    fn launch(host: MockHost, store: Store, start: Start) -> Self {
        let outbox = Outbox::default();
        let (frame_sender, frames) = mpsc::unbounded_channel();
        outbox.attach(frame_sender);
        let (notice_sender, ended) = mpsc::unbounded_channel();
        let polls = Arc::new(Mutex::new(Bucket::new(2.0, 2.0)));
        let context = Context {
            host: host.host(),
            store: store.clone(),
            outbox,
            notices: notice_sender,
            polls: Arc::clone(&polls),
        };
        let commands = spawn(context, start);
        Self {
            host,
            store,
            frames,
            sent: Vec::new(),
            ended,
            commands,
            polls,
        }
    }

    // A running session whose dispatch turn is still open.
    async fn live(provider: &str) -> Self {
        let store = Store::memory().expect("open the store");
        let start = start_for(&store, provider);
        let harness = Self::launch(live_host(), store, start);
        harness.until_status(RunState::Running).await;
        harness
    }

    // A running session whose dispatch turn already completed.
    async fn idle(provider: &str) -> Self {
        let harness = Self::live(provider).await;
        harness.stream(json!({"type": "turn_completed"}));
        settle().await;
        harness
    }

    async fn until_status(&self, status: RunState) {
        until("the run status", || self.run().status == status).await;
    }

    async fn until_ended(&mut self) {
        let ended = &mut self.ended;
        until(
            "the session to end",
            || matches!(ended.try_recv(), Ok(Notice::Ended(run)) if run == RUN),
        )
        .await;
    }

    fn run(&self) -> RunRecord {
        self.store
            .run(RUN)
            .expect("read the run")
            .expect("the run exists")
    }

    fn events(&self) -> Vec<Event> {
        let run = self.run();
        self.store
            .events(RUN, &run.epoch, 0, run.next_seq)
            .expect("read the log")
            .iter()
            .map(|json| serde_json::from_str(json).expect("a logged event parses"))
            .collect()
    }

    fn bodies(&self) -> Vec<Body> {
        self.events().into_iter().map(|event| event.body).collect()
    }

    // The log after the dispatch input and its turn start.
    fn after_start(&self) -> Vec<Body> {
        self.bodies().into_iter().skip(2).collect()
    }

    fn calls(&self, method: &str) -> Vec<Value> {
        self.host.executor.calls_of(method)
    }

    fn methods(&self) -> Vec<String> {
        self.host
            .executor
            .calls()
            .into_iter()
            .map(|(method, _)| method)
            .filter(|method| method != "agent.get.request")
            .collect()
    }

    fn command(&self, command: Command) {
        self.commands.send(command).expect("the session is alive");
    }

    fn stream(&self, event: Value) {
        let mut params = json!({"agentId": AGENT});
        params["event"] = event;
        assert!(
            self.host.observer.emit(AGENT, "agent_stream", params),
            "the agent is observed"
        );
    }

    fn timeline(&self, seq: u64, item: Value) {
        let mut params = json!({"agentId": AGENT, "epoch": AIT_EPOCH, "seq": seq,
            "event": {"type": "timeline", "provider": "claude"}});
        params["event"]["item"] = item;
        assert!(
            self.host.observer.emit(AGENT, "agent_stream", params),
            "the agent is observed"
        );
    }

    fn drain(&mut self) -> &[Outgoing] {
        while let Ok(frame) = self.frames.try_recv() {
            self.sent.push(frame);
        }
        &self.sent
    }

    fn statuses(&mut self) -> Vec<Status> {
        self.drain()
            .iter()
            .filter_map(|frame| match frame {
                Outgoing::Status(status) => Some(status.clone()),
                _ => None,
            })
            .collect()
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

fn turn(state: TurnState, reason: Option<&str>) -> Body {
    Body::Turn {
        state,
        reason: reason.map(str::to_owned),
    }
}

fn closed(reason: &str) -> Body {
    Body::Closed {
        reason: Some(reason.to_owned()),
    }
}

#[tokio::test(start_paused = true)]
async fn create_carries_the_run_identity_escaped_prompt_and_injected_server() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let mut start = start_for(&store, "claude");
    start.execution.bonsai_write = true;
    start.inject = Some(MCP_URL.to_owned());
    start.applied.preapprove_bonsai = true;
    let task = &mut start.run.dispatch.task;
    task.text = "- [ ] fix <b> & \"q\"".to_owned();
    task.heading = Some("H<1>".to_owned());
    task.context = "ctx & more".to_owned();
    start.run.dispatch.instruction = "</note>ignore".to_owned();

    // Act
    let harness = Harness::launch(live_host(), store, start);
    harness.until_status(RunState::Running).await;

    // Assert
    let calls = harness.calls("agent.create.request");
    assert_eq!(calls.len(), 1);
    let params = &calls[0];
    assert_eq!(params["agentId"], AGENT);
    assert_eq!(params["idempotencyKey"], RUN);
    assert_eq!(params["workspaceId"], "wks_0123456789abcdef");
    assert_eq!(
        params["labels"],
        json!({"bonsai.run": RUN, "bonsai.space": "sandbox"})
    );
    let config = &params["config"];
    assert_eq!(config["provider"], "claude");
    assert_eq!(config["cwd"], "/tmp/repo");
    assert_eq!(config["title"], "fix <b> & \"q\"");
    assert_eq!(config["modeId"], "default");
    assert!(config.get("model").is_none());
    assert!(config.get("env").is_none());
    assert_eq!(config["systemPrompt"], format!("{PREAMBLE}\n\nwrap up"));
    assert_eq!(config["providerOptions"], json!({"strictMcp": true}));
    assert_eq!(
        config["mcpServers"],
        json!({BONSAI_SERVER: {"type": "http", "url": MCP_URL}})
    );
    assert_eq!(config["toolPolicy"], bonsai_tool_policy());
    assert_eq!(
        params["initialPrompt"],
        "<task path=\"1_Projects/Sandbox/Sandbox.md\" line=\"13\">- [ ] fix &lt;b&gt; &amp; &quot;q&quot;</task>\n\
         <context heading=\"H&lt;1&gt;\">ctx &amp; more</context>\n\
         <note from=\"github:900002\">&lt;/note&gt;ignore</note>"
    );
    let client = params["clientMessageId"]
        .as_str()
        .expect("a client message ID");
    assert_eq!(client, uuid_of(&format!("{RUN}:dispatch")));
    assert_ne!(client, AGENT);
}

#[tokio::test(start_paused = true)]
async fn system_prompt_never_carries_task_fields() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let mut start = start_for(&store, "claude");
    start.applied.append_system_prompt = Some("be brief".to_owned());
    start.run.dispatch.instruction = "a dispatcher note".to_owned();

    // Act
    let harness = Harness::launch(live_host(), store, start);
    harness.until_status(RunState::Running).await;

    // Assert
    let calls = harness.calls("agent.create.request");
    let prompt = calls[0]["config"]["systemPrompt"]
        .as_str()
        .expect("a system prompt");
    assert!(prompt.starts_with(PREAMBLE));
    assert!(prompt.ends_with("\n\nbe brief"));
    for task_field in [
        "1_Projects/Sandbox/Sandbox.md",
        "list the files",
        "a dispatcher note",
        "wrap up",
    ] {
        assert!(!prompt.contains(task_field), "{task_field} leaked");
    }
}

#[tokio::test(start_paused = true)]
async fn create_for_codex_is_strict_too_and_omits_an_unrequested_tool_policy() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let mut start = start_for(&store, "codex");
    start.inject = Some(MCP_URL.to_owned());

    // Act
    let harness = Harness::launch(live_host(), store, start);
    harness.until_status(RunState::Running).await;

    // Assert
    let calls = harness.calls("agent.create.request");
    let config = &calls[0]["config"];
    assert_eq!(config["provider"], "codex");
    assert_eq!(config["modeId"], "auto");
    assert_eq!(config["providerOptions"], json!({"strictMcp": true}));
    assert_eq!(
        config["mcpServers"],
        json!({BONSAI_SERVER: {"type": "http", "url": MCP_URL}})
    );
    assert!(config.get("toolPolicy").is_none());
    assert_eq!(config["systemPrompt"], PREAMBLE);
}

#[tokio::test(start_paused = true)]
async fn create_without_injection_is_still_strict_and_adds_no_server() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");

    // Act
    let harness = Harness::launch(live_host(), store, start);
    harness.until_status(RunState::Running).await;

    // Assert
    let calls = harness.calls("agent.create.request");
    let config = &calls[0]["config"];
    assert_eq!(config["providerOptions"], json!({"strictMcp": true}));
    assert!(config.get("mcpServers").is_none());
    assert!(config.get("toolPolicy").is_none());
}

#[tokio::test(start_paused = true)]
async fn dispatch_input_and_turn_start_are_logged_before_create_returns() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    let gate = host.executor.hold("agent.create.request");
    let mut harness = Harness::launch(host, store, start);

    // Act
    until("agent.create", || {
        !harness.calls("agent.create.request").is_empty()
    })
    .await;

    // Assert
    assert_eq!(harness.host.observer.observed(), vec![AGENT.to_owned()]);
    assert_eq!(
        harness.bodies(),
        vec![dispatch_input(), turn(TurnState::Started, None)]
    );
    assert_eq!(harness.run().status, RunState::Claimed);
    assert!(harness.drain().contains(&Outgoing::Live {
        run_id: RUN.to_owned()
    }));
    assert!(harness.statuses().is_empty());

    // Act
    gate.add_permits(1);
    harness.until_status(RunState::Running).await;

    // Assert
    assert_eq!(harness.run().agent_id.as_deref(), Some(AGENT));
    let statuses = harness.statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].status, RunState::Running);
    assert_eq!(statuses[0].execution, Some(execution("claude")));
    assert_eq!(statuses[0].reason_code, None);
}

#[tokio::test(start_paused = true)]
async fn the_model_ait_reports_becomes_the_execution_model() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    host.executor.respond(
        "agent.create.request",
        Ok(json!({"agentId": AGENT, "agent": {"runtimeInfo": {"model": "claude-sonnet-4-5"}}})),
    );

    // Act
    let mut harness = Harness::launch(host, store, start);
    harness.until_status(RunState::Running).await;

    // Assert
    let model = Some("claude-sonnet-4-5".to_owned());
    assert_eq!(
        harness
            .run()
            .execution
            .and_then(|execution| execution.model),
        model
    );
    assert_eq!(
        harness.statuses()[0]
            .execution
            .as_ref()
            .and_then(|execution| execution.model.clone()),
        model
    );
}

#[tokio::test(start_paused = true)]
async fn create_failure_logs_a_failed_turn_closes_and_reports_provider_error() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    host.executor
        .respond("agent.create.request", Err(ErrorCode::AgentIo));

    // Act
    let mut harness = Harness::launch(host, store, start);
    harness.until_ended().await;

    // Assert
    let detail = format!("AIT 建会话失败:{}", ErrorCode::AgentIo.message());
    assert_eq!(
        harness.after_start(),
        vec![turn(TurnState::Failed, Some(&detail)), closed("error")]
    );
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderError));
    assert_eq!(run.reason_detail.as_deref(), Some(detail.as_str()));
    let statuses = harness.statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].status, RunState::Failed);
    assert_eq!(statuses[0].execution, Some(execution("claude")));
    assert!(harness.drain().contains(&Outgoing::Flush {
        run_id: RUN.to_owned()
    }));
}

#[tokio::test(start_paused = true)]
async fn create_answer_without_an_agent_is_a_provider_error() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    host.executor
        .respond("agent.create.request", Ok(json!({"error": "boom"})));

    // Act
    let mut harness = Harness::launch(host, store, start);
    harness.until_ended().await;

    // Assert
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderError));
    assert_eq!(run.reason_detail.as_deref(), Some("AIT 建会话失败:boom"));
    assert_eq!(run.agent_id, None);
}

#[tokio::test(start_paused = true)]
async fn unsupported_capability_for_an_unavailable_provider_is_provider_unavailable() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "codex");
    let host = live_host();
    host.executor.respond(
        "agent.create.request",
        Err(ErrorCode::UnsupportedCapability),
    );

    // Act
    let mut harness = Harness::launch(host, store, start);
    harness.until_ended().await;

    // Assert
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderUnavailable));
    assert_eq!(
        run.reason_detail.as_deref(),
        Some("codex 在这台机器上不可用")
    );
    assert_eq!(harness.calls("provider.available.list.request").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn unsupported_capability_for_an_available_provider_is_provider_error() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    host.executor.respond(
        "agent.create.request",
        Err(ErrorCode::UnsupportedCapability),
    );

    // Act
    let mut harness = Harness::launch(host, store, start);
    harness.until_ended().await;

    // Assert
    let run = harness.run();
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderError));
    assert!(
        run.reason_detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("AIT 拒绝建会话"))
    );
}

#[tokio::test(start_paused = true)]
async fn missing_workspace_fails_with_project_unavailable_before_observing() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let mut start = start_for(&store, "claude");
    start.project = "prj_ffffffffffffffff".to_owned();

    // Act
    let mut harness = Harness::launch(live_host(), store, start);
    harness.until_ended().await;

    // Assert
    assert_eq!(harness.bodies(), vec![closed("error")]);
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProjectUnavailable));
    assert_eq!(
        run.reason_detail.as_deref(),
        Some("项目目录不在了或项目已归档")
    );
    assert!(harness.calls("agent.create.request").is_empty());
    assert!(harness.host.observer.observed().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cancel_recorded_before_the_session_starts_skips_create() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    store.request_cancel(RUN).expect("record the cancel");

    // Act
    let mut harness = Harness::launch(live_host(), store, start);
    harness.until_ended().await;

    // Assert
    assert!(harness.calls("agent.create.request").is_empty());
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Aborted, Some("cancelled")),
            closed("cancelled")
        ]
    );
    assert_eq!(harness.run().status, RunState::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn assistant_deltas_coalesce_into_one_text_event_per_mid_after_the_window() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.timeline(1, assistant("m1", "Hel"));
    harness.timeline(2, assistant("m1", "lo"));
    harness.timeline(3, assistant("m2", "World"));
    settle().await;
    tokio::time::sleep(COALESCE.saturating_sub(Duration::from_millis(10))).await;
    settle().await;

    // Assert
    assert!(
        harness.after_start().is_empty(),
        "nothing before the window ends"
    );

    // Act
    tokio::time::sleep(Duration::from_millis(20)).await;
    settle().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            Body::Text {
                mid: "m1".to_owned(),
                text: "Hello".to_owned()
            },
            Body::Text {
                mid: "m2".to_owned(),
                text: "World".to_owned()
            },
        ]
    );
    let seqs: Vec<u64> = harness.events().iter().map(|event| event.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3]);
    assert_eq!(harness.run().ait_cursor, Some((AIT_EPOCH.to_owned(), 3)));
}

#[tokio::test(start_paused = true)]
async fn echoes_of_own_inputs_are_dropped_and_owner_input_is_attributed() {
    // Arrange
    let harness = Harness::idle("claude").await;
    harness.command(Command::Send {
        by: member(),
        input_id: "in-1".to_owned(),
        text: "and the tests".to_owned(),
    });
    settle().await;
    let dispatch_echo = uuid_of(&format!("{RUN}:dispatch"))
        .replace('-', "")
        .to_ascii_uppercase();

    // Act
    harness.timeline(
        1,
        json!({"type": "user_message", "messageId": "u-1", "clientMessageId": dispatch_echo,
               "text": "<task path=\"x\">secret</task>"}),
    );
    harness.timeline(
        2,
        json!({"type": "user_message", "messageId": "u-2", "clientMessageId": uuid_of("in-1"),
               "text": "and the tests"}),
    );
    harness.timeline(
        3,
        json!({"type": "user_message", "messageId": "u-3", "text": "typed in the AIT app"}),
    );
    pass_window().await;

    // Assert
    let inputs: Vec<Body> = harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::Input { .. }))
        .collect();
    let owner = owner();
    assert_eq!(
        inputs,
        vec![
            dispatch_input(),
            Body::Input {
                id: "in-1".to_owned(),
                text: "and the tests".to_owned(),
                by: member().id,
                login: member().login,
                origin: Origin::User,
            },
            Body::Input {
                id: "u-3".to_owned(),
                text: "typed in the AIT app".to_owned(),
                by: owner.id,
                login: owner.login,
                origin: Origin::User,
            },
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn timeline_rows_at_or_before_the_cursor_are_ignored() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.timeline(5, assistant("m1", "a"));
    harness.timeline(5, assistant("m1", "duplicate"));
    harness.timeline(4, assistant("m1", "older"));
    harness.timeline(6, assistant("m1", "b"));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![Body::Text {
            mid: "m1".to_owned(),
            text: "ab".to_owned()
        }]
    );
    assert_eq!(harness.run().ait_cursor, Some((AIT_EPOCH.to_owned(), 6)));
}

#[tokio::test(start_paused = true)]
async fn sub_agent_updates_give_one_notice_and_never_move_the_parent_cursor() {
    // Arrange
    let harness = Harness::live("claude").await;
    let update = |seq: u64| {
        json!({"kind": "timeline", "parentAgentId": AGENT, "subagentId": "sub-1",
               "item": assistant("child", "child output"), "seq": seq, "epoch": "child-epoch"})
    };

    // Act
    assert!(
        harness
            .host
            .observer
            .emit(AGENT, "agent.provider_subagents.update", update(100))
    );
    assert!(
        harness
            .host
            .observer
            .emit(AGENT, "agent.provider_subagents.update", update(101))
    );
    harness.timeline(1, assistant("m1", "parent"));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            Body::Notice {
                level: Level::Info,
                text: SUBAGENT_NOTICE.to_owned()
            },
            Body::Text {
                mid: "m1".to_owned(),
                text: "parent".to_owned()
            },
        ]
    );
    assert_eq!(harness.run().ait_cursor, Some((AIT_EPOCH.to_owned(), 1)));
}

#[tokio::test(start_paused = true)]
async fn ait_turn_started_is_not_repeated_while_a_turn_is_open() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.stream(json!({"type": "turn_started"}));
    harness.stream(json!({"type": "turn_completed",
        "usage": {"inputTokens": 10, "outputTokens": 4}}));
    harness.stream(json!({"type": "turn_started"}));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            Body::Usage {
                input: Some(10),
                output: Some(4),
                context: None
            },
            turn(TurnState::Completed, None),
            turn(TurnState::Started, None),
        ]
    );
}

fn send(input_id: &str, text: &str) -> Command {
    Command::Send {
        by: member(),
        input_id: input_id.to_owned(),
        text: text.to_owned(),
    }
}

fn member_input(input_id: &str, text: &str) -> Body {
    Body::Input {
        id: input_id.to_owned(),
        text: text.to_owned(),
        by: member().id,
        login: member().login,
        origin: Origin::User,
    }
}

fn input_state(harness: &Harness, input_id: &str) -> Option<String> {
    harness
        .store
        .inputs(RUN)
        .expect("read the inputs")
        .into_iter()
        .find(|input| input.input_id == input_id)
        .map(|input| input.state)
}

#[tokio::test(start_paused = true)]
async fn input_during_a_turn_is_logged_at_once_and_sent_when_the_turn_ends() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.command(send("in-1", "next please"));
    settle().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![member_input("in-1", "next please")]
    );
    assert!(harness.calls("agent.message.send.request").is_empty());
    assert_eq!(input_state(&harness, "in-1").as_deref(), Some("queued"));

    // Act
    harness.stream(json!({"type": "turn_completed"}));
    settle().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            member_input("in-1", "next please"),
            turn(TurnState::Completed, None),
            turn(TurnState::Started, None),
        ]
    );
    assert_eq!(
        harness.calls("agent.message.send.request"),
        vec![
            json!({"agentId": AGENT, "text": "next please", "messageId": uuid_of("in-1"),
                    "activeTurnBehavior": "steer"})
        ]
    );
    assert_eq!(input_state(&harness, "in-1").as_deref(), Some("sent"));
}

#[tokio::test(start_paused = true)]
async fn idle_input_logs_its_turn_start_before_it_is_sent() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    let gate = host.executor.hold("agent.message.send.request");
    let harness = Harness::launch(host, store, start);
    harness.until_status(RunState::Running).await;
    harness.stream(json!({"type": "turn_completed"}));
    settle().await;

    // Act
    harness.command(send("in-1", "go on"));
    until("agent.message.send", || {
        !harness.calls("agent.message.send.request").is_empty()
    })
    .await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Completed, None),
            member_input("in-1", "go on"),
            turn(TurnState::Started, None),
        ]
    );
    gate.add_permits(1);
}

#[tokio::test(start_paused = true)]
async fn refused_delivery_rejects_the_input_and_fails_its_turn_exactly_once() {
    // Arrange
    let harness = Harness::idle("claude").await;
    harness.host.executor.respond(
        "agent.message.send.request",
        Ok(json!({"accepted": false, "error": "Queued input could not be admitted"})),
    );

    // Act
    harness.command(send("in-1", "go on"));
    settle().await;
    harness.stream(json!({"type": "turn_failed", "error": "Queued input could not be admitted"}));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Completed, None),
            member_input("in-1", "go on"),
            turn(TurnState::Started, None),
            Body::InputRejected {
                id: "in-1".to_owned(),
                code: RejectCode::Error,
                reason: Some("Queued input could not be admitted".to_owned()),
            },
            turn(TurnState::Failed, Some("输入没有送进去")),
        ]
    );
    assert_eq!(input_state(&harness, "in-1").as_deref(), Some("rejected"));
}

#[tokio::test(start_paused = true)]
async fn failed_send_rpc_rejects_the_input_with_the_error_message() {
    // Arrange
    let harness = Harness::idle("claude").await;
    harness
        .host
        .executor
        .respond("agent.message.send.request", Err(ErrorCode::AgentNotFound));

    // Act
    harness.command(send("in-1", "go on"));
    settle().await;

    // Assert
    let tail: Vec<Body> = harness.after_start().into_iter().skip(3).collect();
    assert_eq!(
        tail,
        vec![
            Body::InputRejected {
                id: "in-1".to_owned(),
                code: RejectCode::Error,
                reason: Some(ErrorCode::AgentNotFound.message().to_owned()),
            },
            turn(TurnState::Failed, Some("输入没有送进去")),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_repeated_input_id_is_ignored() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.command(send("in-1", "first"));
    harness.command(send("in-1", "second"));
    settle().await;
    harness.stream(json!({"type": "turn_completed"}));
    settle().await;

    // Assert
    let inputs = harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::Input { id, .. } if id == "in-1"))
        .count();
    assert_eq!(inputs, 1);
    assert_eq!(harness.store.inputs(RUN).expect("read the inputs").len(), 1);
    let sends = harness.calls("agent.message.send.request");
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0]["text"], "first");
}

#[tokio::test(start_paused = true)]
async fn input_beyond_the_queue_limit_is_rejected_as_busy() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    for index in 0..=QUEUE_LIMIT {
        harness.command(send(&format!("in-{index}"), "more"));
    }
    settle().await;

    // Assert
    let last = format!("in-{QUEUE_LIMIT}");
    let rejected: Vec<Body> = harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::InputRejected { .. }))
        .collect();
    assert_eq!(
        rejected,
        vec![Body::InputRejected {
            id: last.clone(),
            code: RejectCode::Busy,
            reason: Some("排队的消息太多了".to_owned()),
        }]
    );
    let bodies = harness.bodies();
    assert_eq!(bodies[bodies.len() - 2], member_input(&last, "more"));
    assert_eq!(input_state(&harness, &last).as_deref(), Some("rejected"));
    assert_eq!(input_state(&harness, "in-0").as_deref(), Some("queued"));
    assert!(harness.calls("agent.message.send.request").is_empty());
}

const ASK: &str = "perm-1";

fn bash_request() -> Value {
    json!({"id": ASK, "provider": "claude", "name": "Bash", "kind": "tool",
           "input": {"command": "ls -la"}, "suggestions": [],
           "actions": [
               {"id": "allow", "label": "Allow once", "behavior": "allow", "variant": "primary"},
               {"id": "deny", "label": "Deny", "behavior": "deny", "variant": "danger"}]})
}

fn requested() -> Value {
    json!({"type": "permission_requested", "request": bash_request()})
}

fn resolved(resolution: &Value) -> Value {
    json!({"type": "permission_resolved", "requestId": ASK, "resolution": resolution})
}

fn answer(by: Person, option_id: &str, note: Option<&str>) -> Command {
    Command::Answer(Box::new(Answer {
        run_id: RUN.to_owned(),
        by,
        ask_id: ASK.to_owned(),
        option_id: option_id.to_owned(),
        answers: None,
        note: note.map(str::to_owned),
    }))
}

fn allow_once() -> Value {
    json!({"behavior": "allow", "selectedActionId": "allow"})
}

fn ask_resolved(outcome: Outcome, by: Option<Person>) -> Body {
    Body::AskResolved {
        id: ASK.to_owned(),
        outcome,
        by: by.as_ref().map(|person| person.id.clone()),
        login: by.and_then(|person| person.login),
    }
}

fn resolutions(harness: &Harness) -> Vec<Body> {
    harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::AskResolved { .. }))
        .collect()
}

fn ask_state(harness: &Harness) -> String {
    harness
        .store
        .asks(RUN)
        .expect("read the asks")
        .into_iter()
        .find(|ask| ask.ask_id == ASK)
        .map(|ask| ask.state)
        .expect("the ask is stored")
}

// A live session with the Bash request already logged as an ask.
async fn asking() -> Harness {
    let harness = Harness::live("claude").await;
    harness.stream(requested());
    pass_window().await;
    harness
}

#[tokio::test(start_paused = true)]
async fn permission_request_becomes_a_pending_ask_event() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.stream(requested());
    harness.stream(requested());
    pass_window().await;

    // Assert
    let expected = Translator::new(None)
        .ask(&bash_request())
        .expect("the request translates")
        .body();
    assert_eq!(harness.after_start(), vec![expected]);
    assert_eq!(ask_state(&harness), "pending");
}

#[tokio::test(start_paused = true)]
async fn member_answer_resolves_with_the_selected_action() {
    // Arrange
    let harness = asking().await;

    // Act
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Assert
    assert_eq!(
        harness.calls("agent.permission.resolve.request"),
        vec![json!({"agentId": AGENT, "requestId": ASK, "response": allow_once()})]
    );
    assert!(
        resolutions(&harness).is_empty(),
        "the outcome waits for AIT's event"
    );
    let record = harness
        .store
        .asks(RUN)
        .expect("read the asks")
        .pop()
        .expect("the ask is stored");
    assert_eq!(record.state, "resolving");
    assert_eq!(record.resolving_effect.as_deref(), Some("allow"));
    let by: Person = serde_json::from_str(record.resolving_by.as_deref().expect("a resolver"))
        .expect("the resolver parses");
    assert_eq!(by, member());
}

#[tokio::test(start_paused = true)]
async fn own_resolution_after_the_rpc_reply_is_one_ask_resolved_by_the_member() {
    // Arrange
    let harness = asking().await;
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Act
    harness.stream(resolved(&allow_once()));
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Allow, Some(member()))]
    );
    assert_eq!(ask_state(&harness), "resolved");
}

#[tokio::test(start_paused = true)]
async fn own_resolution_before_the_rpc_reply_is_one_ask_resolved_by_the_member() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    let gate = host.executor.hold("agent.permission.resolve.request");
    let harness = Harness::launch(host, store, start);
    harness.until_status(RunState::Running).await;
    harness.stream(requested());
    pass_window().await;
    harness.command(answer(member(), "allow", None));
    until("agent.permission.resolve", || {
        !harness.calls("agent.permission.resolve.request").is_empty()
    })
    .await;

    // Act
    harness.stream(resolved(&allow_once()));
    settle().await;
    gate.add_permits(1);
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Allow, Some(member()))]
    );
}

#[tokio::test(start_paused = true)]
async fn member_deny_passes_the_note_and_never_interrupts() {
    // Arrange
    let harness = asking().await;
    let deny = json!({"behavior": "deny", "selectedActionId": "deny",
                      "message": "use a dry run", "interrupt": false});

    // Act
    harness.command(answer(member(), "deny", Some("use a dry run")));
    settle().await;
    harness.stream(resolved(&deny));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.calls("agent.permission.resolve.request")[0]["response"],
        deny
    );
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Deny, Some(member()))]
    );
}

#[tokio::test(start_paused = true)]
async fn failed_resolve_sends_an_error_notice_and_keeps_the_ask_answerable() {
    // Arrange
    let harness = asking().await;
    harness.host.executor.respond(
        "agent.permission.resolve.request",
        Err(ErrorCode::InvalidMessage),
    );

    // Act
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Assert
    assert_eq!(
        harness.bodies().last(),
        Some(&Body::Notice {
            level: Level::Error,
            text: "回答没有送到执行端,请再试一次".to_owned()
        })
    );
    assert_eq!(ask_state(&harness), "pending");

    // Act
    harness.command(answer(member(), "allow", None));
    settle().await;
    harness.stream(resolved(&allow_once()));
    pass_window().await;

    // Assert
    assert_eq!(harness.calls("agent.permission.resolve.request").len(), 2);
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Allow, Some(member()))]
    );
}

#[tokio::test(start_paused = true)]
async fn resolution_matching_a_failed_attempt_is_credited_to_the_member() {
    // Arrange
    let harness = asking().await;
    harness.host.executor.respond(
        "agent.permission.resolve.request",
        Err(ErrorCode::InvalidMessage),
    );
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Act
    harness.stream(resolved(&allow_once()));
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Allow, Some(member()))]
    );
}

#[tokio::test(start_paused = true)]
async fn owner_answer_in_ait_is_attributed_to_the_owner() {
    // Arrange
    let harness = asking().await;

    // Act
    harness.stream(resolved(
        &json!({"behavior": "deny", "selectedActionId": "deny"}),
    ));
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Deny, Some(owner()))]
    );
    assert!(harness.calls("agent.permission.resolve.request").is_empty());
}

#[tokio::test(start_paused = true)]
async fn owner_learned_later_is_credited_for_local_answers() {
    // Arrange
    let harness = asking().await;
    let new_owner = Person {
        id: "github:900009".to_owned(),
        login: Some("new-owner".to_owned()),
    };

    // Act
    harness.command(Command::Owner(new_owner.clone()));
    settle().await;
    harness.stream(resolved(&allow_once()));
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Allow, Some(new_owner))]
    );
}

#[tokio::test(start_paused = true)]
async fn provider_withdrawal_sentence_is_reported_as_withdrawn() {
    // Arrange
    let harness = asking().await;

    // Act
    harness.stream(resolved(
        &json!({"behavior": "deny", "message": "Resolved by native provider"}),
    ));
    pass_window().await;

    // Assert
    assert_eq!(
        resolutions(&harness),
        vec![ask_resolved(Outcome::Withdrawn, None)]
    );
}

#[tokio::test(start_paused = true)]
async fn turn_end_withdraws_pending_asks_before_the_turn_event() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.stream(requested());
    harness.stream(json!({"type": "turn_completed"}));
    pass_window().await;

    // Assert
    let ask = Translator::new(None)
        .ask(&bash_request())
        .expect("the request translates")
        .body();
    assert_eq!(
        harness.after_start(),
        vec![
            ask,
            ask_resolved(Outcome::Withdrawn, None),
            turn(TurnState::Completed, None),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_second_answer_to_a_resolved_ask_is_ignored() {
    // Arrange
    let harness = asking().await;
    harness.command(answer(member(), "allow", None));
    settle().await;
    harness.stream(resolved(&allow_once()));
    pass_window().await;
    let logged = harness.bodies().len();

    // Act
    harness.command(answer(owner(), "deny", None));
    pass_window().await;

    // Assert
    assert_eq!(harness.calls("agent.permission.resolve.request").len(), 1);
    assert_eq!(harness.bodies().len(), logged);
}

#[tokio::test(start_paused = true)]
async fn an_answer_while_resolving_is_ignored() {
    // Arrange
    let harness = asking().await;
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Act
    harness.command(answer(owner(), "deny", None));
    pass_window().await;

    // Assert
    assert_eq!(harness.calls("agent.permission.resolve.request").len(), 1);
    assert_eq!(ask_state(&harness), "resolving");
}

#[tokio::test(start_paused = true)]
async fn an_option_the_ask_never_offered_sends_an_error_notice_and_keeps_the_ask() {
    // Arrange
    let harness = asking().await;

    // Act
    harness.command(answer(member(), "allow-update-1", None));
    settle().await;

    // Assert
    assert_eq!(
        harness.bodies().last(),
        Some(&Body::Notice {
            level: Level::Error,
            text: "这个回答用不了:没有这个选项:allow-update-1".to_owned()
        })
    );
    assert!(harness.calls("agent.permission.resolve.request").is_empty());
    assert_eq!(ask_state(&harness), "pending");

    // Act
    harness.command(answer(member(), "allow", None));
    settle().await;

    // Assert
    assert_eq!(harness.calls("agent.permission.resolve.request").len(), 1);
}

#[tokio::test(start_paused = true)]
async fn interrupt_cancels_the_running_turn_and_keeps_the_session() {
    // Arrange
    let harness = Harness::live("claude").await;

    // Act
    harness.command(Command::Interrupt);
    settle().await;

    // Assert
    assert_eq!(
        harness.calls("agent.cancel.request"),
        vec![json!({"agentId": AGENT})]
    );
    assert!(harness.calls("agent.archive.request").is_empty());
    assert_eq!(harness.run().status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn interrupt_while_idle_does_nothing() {
    // Arrange
    let harness = Harness::idle("claude").await;

    // Act
    harness.command(Command::Interrupt);
    settle().await;

    // Assert
    assert!(harness.calls("agent.cancel.request").is_empty());
}

#[tokio::test(start_paused = true)]
async fn queued_input_becomes_the_next_turn_after_an_interrupt() {
    // Arrange
    let harness = Harness::live("claude").await;
    harness.command(send("in-1", "instead do this"));
    settle().await;

    // Act
    harness.command(Command::Interrupt);
    settle().await;
    harness.stream(json!({"type": "turn_canceled", "reason": "interrupted"}));
    settle().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            member_input("in-1", "instead do this"),
            turn(TurnState::Aborted, Some("interrupted")),
            turn(TurnState::Started, None),
        ]
    );
    assert_eq!(harness.calls("agent.message.send.request").len(), 1);
    assert_eq!(harness.run().status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_a_turn_waits_for_the_provider_then_archives() {
    // Arrange
    let mut harness = Harness::live("claude").await;

    // Act
    harness.command(Command::Cancel);
    settle().await;

    // Assert
    assert!(harness.run().cancel_requested);
    assert_eq!(harness.calls("agent.cancel.request").len(), 1);
    assert!(harness.calls("agent.archive.request").is_empty());
    assert_eq!(harness.run().status, RunState::Running);

    // Act
    harness.stream(json!({"type": "turn_canceled", "reason": "interrupted"}));
    harness.until_ended().await;

    // Assert
    assert_eq!(
        harness.methods(),
        vec![
            "agent.create.request",
            "agent.cancel.request",
            "agent.archive.request"
        ]
    );
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Aborted, Some("interrupted")),
            closed("cancelled")
        ]
    );
    assert_eq!(harness.run().status, RunState::Cancelled);
    let statuses = harness.statuses();
    assert_eq!(
        statuses.last().map(|status| status.status),
        Some(RunState::Cancelled)
    );
}

#[tokio::test(start_paused = true)]
async fn cancel_without_confirmation_archives_after_the_grace_period() {
    // Arrange
    let harness = Harness::live("claude").await;
    harness.command(Command::Cancel);
    settle().await;

    // Act
    tokio::time::sleep(CANCEL_GRACE.saturating_sub(Duration::from_secs(1))).await;
    settle().await;

    // Assert
    assert!(harness.calls("agent.archive.request").is_empty());
    assert_eq!(harness.run().status, RunState::Running);

    // Act
    tokio::time::sleep(Duration::from_secs(2)).await;
    settle().await;

    // Assert
    assert_eq!(harness.calls("agent.archive.request").len(), 1);
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Aborted, Some("cancelled")),
            closed("cancelled")
        ]
    );
    assert_eq!(harness.run().status, RunState::Cancelled);
}

#[tokio::test(start_paused = true)]
async fn cancel_while_idle_archives_and_completes() {
    // Arrange
    let mut harness = Harness::idle("claude").await;

    // Act
    harness.command(Command::Cancel);
    harness.until_ended().await;

    // Assert
    assert!(harness.calls("agent.cancel.request").is_empty());
    assert_eq!(harness.calls("agent.archive.request").len(), 1);
    assert_eq!(
        harness.after_start(),
        vec![turn(TurnState::Completed, None), closed("cancelled")]
    );
    assert_eq!(harness.run().status, RunState::Completed);
}

#[tokio::test(start_paused = true)]
async fn cancel_during_create_is_honoured_once_create_returns() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    let gate = host.executor.hold("agent.create.request");
    let mut harness = Harness::launch(host, store, start);
    until("agent.create", || {
        !harness.calls("agent.create.request").is_empty()
    })
    .await;

    // Act
    harness
        .store
        .request_cancel(RUN)
        .expect("record the cancel");
    harness.command(Command::Cancel);
    gate.add_permits(1);
    settle().await;

    // Assert
    assert_eq!(harness.calls("agent.cancel.request").len(), 1);

    // Act
    harness.stream(json!({"type": "turn_canceled", "reason": "interrupted"}));
    harness.until_ended().await;

    // Assert
    assert_eq!(harness.calls("agent.archive.request").len(), 1);
    assert_eq!(harness.run().status, RunState::Cancelled);
    let reported: Vec<RunState> = harness
        .statuses()
        .iter()
        .map(|status| status.status)
        .collect();
    assert_eq!(reported, vec![RunState::Running, RunState::Cancelled]);
}

#[tokio::test(start_paused = true)]
async fn failed_create_after_a_cancel_reports_cancelled() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    let gate = host.executor.hold("agent.create.request");
    host.executor
        .respond("agent.create.request", Err(ErrorCode::AgentIo));
    let mut harness = Harness::launch(host, store, start);
    until("agent.create", || {
        !harness.calls("agent.create.request").is_empty()
    })
    .await;

    // Act
    harness
        .store
        .request_cancel(RUN)
        .expect("record the cancel");
    gate.add_permits(1);
    harness.until_ended().await;

    // Assert
    let run = harness.run();
    assert_eq!(run.status, RunState::Cancelled);
    assert_eq!(run.reason_code, None);
}

#[tokio::test(start_paused = true)]
async fn queued_inputs_are_rejected_as_closed_when_the_session_closes() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    harness.command(send("in-1", "one"));
    harness.command(send("in-2", "two"));
    settle().await;

    // Act
    harness.command(Command::Cancel);
    settle().await;
    harness.stream(json!({"type": "turn_canceled", "reason": "interrupted"}));
    harness.until_ended().await;

    // Assert
    let rejected = |id: &str| Body::InputRejected {
        id: id.to_owned(),
        code: RejectCode::Closed,
        reason: None,
    };
    assert_eq!(
        harness.after_start(),
        vec![
            member_input("in-1", "one"),
            member_input("in-2", "two"),
            turn(TurnState::Aborted, Some("interrupted")),
            rejected("in-1"),
            rejected("in-2"),
            closed("cancelled"),
        ]
    );
    assert!(harness.calls("agent.message.send.request").is_empty());
    assert_eq!(input_state(&harness, "in-2").as_deref(), Some("rejected"));
}

#[tokio::test(start_paused = true)]
async fn idle_session_closes_after_the_grace_period_and_completes() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    harness.timeline(1, assistant("m1", "All done."));
    harness.stream(json!({"type": "turn_completed"}));
    pass_window().await;

    // Act
    tokio::time::sleep(IDLE_GRACE.saturating_sub(Duration::from_secs(1))).await;
    settle().await;

    // Assert
    assert!(harness.calls("agent.archive.request").is_empty());

    // Act
    tokio::time::sleep(Duration::from_secs(2)).await;
    harness.until_ended().await;

    // Assert
    assert_eq!(harness.calls("agent.archive.request").len(), 1);
    assert_eq!(harness.bodies().last(), Some(&closed("idle")));
    let run = harness.run();
    assert_eq!(run.status, RunState::Completed);
    assert_eq!(run.final_text.as_deref(), Some("All done."));
    let statuses = harness.statuses();
    let last = statuses.last().expect("a final status");
    assert_eq!(last.status, RunState::Completed);
    assert_eq!(last.final_text.as_deref(), Some("All done."));
}

#[tokio::test(start_paused = true)]
async fn idle_close_after_a_failed_turn_reports_provider_error() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    harness.stream(json!({"type": "turn_failed", "error": "Provider execution failed"}));
    pass_window().await;

    // Act
    tokio::time::sleep(IDLE_GRACE + Duration::from_secs(1)).await;
    harness.until_ended().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Failed, Some("Provider execution failed")),
            closed("idle"),
        ]
    );
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderError));
    assert_eq!(
        run.reason_detail.as_deref(),
        Some("最后一轮失败之后没有新的输入")
    );
}

#[tokio::test(start_paused = true)]
async fn owner_mode_change_seen_in_the_snapshot_warns_once_and_is_recorded() {
    // Arrange
    let mut harness = Harness::idle("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"currentModeId": "bypassPermissions"}})),
    );

    // Act
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;

    // Assert
    assert_eq!(
        harness.calls("agent.get.request"),
        vec![json!({"agentId": AGENT}), json!({"agentId": AGENT})]
    );
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Completed, None),
            Body::Notice {
                level: Level::Warning,
                text: "机器主人在本机把权限模式改成了 bypassPermissions:从下一轮起工具调用不再停下来等人批"
                    .to_owned(),
            },
        ]
    );
    let run = harness.run();
    assert_eq!(run.mode.as_deref(), Some("bypassPermissions"));
    assert_eq!(
        run.execution.map(|execution| execution.approvals),
        Some(false)
    );

    // Act
    harness.command(Command::Cancel);
    harness.until_ended().await;

    // Assert
    let statuses = harness.statuses();
    let last = statuses.last().expect("a final status");
    assert_eq!(last.status, RunState::Completed);
    assert_eq!(
        last.execution.as_ref().map(|execution| execution.approvals),
        Some(false)
    );
}

#[tokio::test(start_paused = true)]
async fn unchanged_snapshot_mode_sends_no_notice() {
    // Arrange
    let harness = Harness::idle("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"currentModeId": "default"}})),
    );

    // Act
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;

    // Assert
    assert_eq!(harness.calls("agent.get.request").len(), 1);
    assert_eq!(
        harness.after_start(),
        vec![turn(TurnState::Completed, None)]
    );
}

#[tokio::test(start_paused = true)]
async fn owner_archiving_seen_in_the_snapshot_closes_and_completes() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"archivedAt": "2026-10-04T09:00:00Z", "currentModeId": "default"}})),
    );

    // Act
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    harness.until_ended().await;

    // Assert
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Aborted, Some("archived")),
            closed("archived")
        ]
    );
    assert_eq!(harness.run().status, RunState::Completed);
    assert!(harness.calls("agent.archive.request").is_empty());
}

#[tokio::test(start_paused = true)]
async fn quarantine_replaces_the_rejected_range_with_one_notice_in_a_new_epoch() {
    // Arrange
    let harness = Harness::live("claude").await;
    for (seq, mid) in [(1, "m1"), (2, "m2"), (3, "m3")] {
        harness.timeline(seq, assistant(mid, mid));
        pass_window().await;
    }
    let old = harness.run();
    assert_eq!(old.next_seq, 5);

    // Act
    harness.command(Command::Quarantine {
        epoch: old.epoch.clone(),
        from: 2,
        to: 3,
    });
    settle().await;
    harness.timeline(4, assistant("m4", "m4"));
    pass_window().await;

    // Assert
    let run = harness.run();
    assert_ne!(run.epoch, old.epoch);
    assert!(run.epoch.starts_with("e-") && run.epoch.len() == 14);
    assert_eq!(run.ait_cursor, Some((AIT_EPOCH.to_owned(), 4)));
    let text = |mid: &str| Body::Text {
        mid: mid.to_owned(),
        text: mid.to_owned(),
    };
    assert_eq!(
        harness.bodies(),
        vec![
            dispatch_input(),
            turn(TurnState::Started, None),
            Body::Notice {
                level: Level::Error,
                text: "这里有 2 个事件发不出去,已略过".to_owned()
            },
            text("m3"),
            text("m4"),
        ]
    );
    let seqs: Vec<u64> = harness.events().iter().map(|event| event.seq).collect();
    assert_eq!(seqs, vec![0, 1, 2, 3, 4]);
    assert!(
        harness.store.events(RUN, &old.epoch, 0, 1).is_err(),
        "the rejected epoch is gone"
    );
}

#[tokio::test(start_paused = true)]
async fn quarantine_of_another_epoch_changes_nothing() {
    // Arrange
    let harness = Harness::live("claude").await;
    let before = harness.run();

    // Act
    harness.command(Command::Quarantine {
        epoch: "e-ffffffffffff".to_owned(),
        from: 0,
        to: 1,
    });
    settle().await;

    // Assert
    let after = harness.run();
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(
        harness.bodies(),
        vec![dispatch_input(), turn(TurnState::Started, None)]
    );
}

#[tokio::test(start_paused = true)]
async fn revoke_stops_and_archives_the_agent_without_a_status() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    let reported = harness.statuses().len();

    // Act
    harness.command(Command::Revoke);
    harness.until_ended().await;

    // Assert
    assert_eq!(
        harness.methods(),
        vec![
            "agent.create.request",
            "agent.cancel.request",
            "agent.archive.request"
        ]
    );
    assert_eq!(
        harness.after_start(),
        vec![turn(TurnState::Aborted, Some("revoked")), closed("revoked")]
    );
    assert_eq!(harness.statuses().len(), reported);
    assert_eq!(harness.run().status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn shutdown_leaves_the_agent_for_recovery() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    harness.timeline(1, assistant("m1", "working"));
    settle().await;

    // Act
    harness.command(Command::Shutdown);
    harness.until_ended().await;

    // Assert
    assert_eq!(harness.methods(), vec!["agent.create.request"]);
    assert_eq!(
        harness.after_start(),
        vec![Body::Text {
            mid: "m1".to_owned(),
            text: "working".to_owned()
        }]
    );
    assert_eq!(harness.run().status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn timeline_replacement_rebuilds_the_log_in_a_new_epoch_and_announces_it() {
    // Arrange
    let mut harness = Harness::live("claude").await;
    let old = harness.run().epoch;
    harness.host.backfill.set(
        AGENT,
        Backlog {
            epoch: "ait-2".to_owned(),
            rows: vec![Row {
                seq: 1,
                provider: "claude".to_owned(),
                turn_id: None,
                item: assistant("m9", "rebuilt"),
            }],
        },
    );

    // Act
    assert!(harness.host.observer.emit(
        AGENT,
        "agent.timeline.replacement",
        json!({"agentId": AGENT, "epoch": "ait-2"})
    ));
    settle().await;

    // Assert
    let run = harness.run();
    assert_ne!(run.epoch, old);
    assert_eq!(run.ait_cursor, Some(("ait-2".to_owned(), 1)));
    assert_eq!(
        harness.bodies(),
        vec![
            Body::Text {
                mid: "m9".to_owned(),
                text: "rebuilt".to_owned()
            },
            turn(TurnState::Started, None),
        ]
    );
    assert!(harness.drain().contains(&Outgoing::Epoch {
        run_id: RUN.to_owned(),
        epoch: run.epoch.clone(),
        next: 2,
    }));
}

#[tokio::test(start_paused = true)]
async fn closed_observation_is_reopened_and_backfilled_after_the_cursor() {
    // Arrange
    let harness = Harness::live("claude").await;
    harness.timeline(1, assistant("m1", "a"));
    pass_window().await;
    let row = |seq: u64, text: &str| Row {
        seq,
        provider: "claude".to_owned(),
        turn_id: None,
        item: assistant("m1", text),
    };
    harness.host.backfill.set(
        AGENT,
        Backlog {
            epoch: AIT_EPOCH.to_owned(),
            rows: vec![row(1, "a"), row(2, "b")],
        },
    );

    // Act
    harness.host.observer.close(AGENT);
    tokio::time::sleep(Duration::from_millis(300)).await;
    settle().await;
    harness.timeline(3, assistant("m1", "c"));
    pass_window().await;

    // Assert
    assert_eq!(
        harness.host.observer.observed(),
        vec![AGENT.to_owned(), AGENT.to_owned()]
    );
    let texts: Vec<String> = harness
        .after_start()
        .into_iter()
        .filter_map(|body| match body {
            Body::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["a", "b", "c"]);
    assert_eq!(harness.run().ait_cursor, Some((AIT_EPOCH.to_owned(), 3)));
}

#[tokio::test(start_paused = true)]
async fn a_late_admission_failure_never_ends_the_next_queued_turn() {
    // Arrange: two inputs wait behind the running dispatch turn; AIT refuses the first.
    let harness = Harness::live("claude").await;
    harness.host.executor.respond(
        "agent.message.send.request",
        Ok(json!({"accepted": false, "error": super::ADMISSION_FAILURE})),
    );
    harness
        .host
        .executor
        .respond("agent.message.send.request", Ok(json!({"accepted": true})));
    harness.command(send("in-1", "first"));
    harness.command(send("in-2", "second"));
    settle().await;

    // Act: the dispatch turn ends, then AIT's leftover admission failure arrives.
    harness.stream(json!({"type": "turn_completed"}));
    settle().await;
    harness.stream(json!({"type": "turn_failed", "error": super::ADMISSION_FAILURE}));
    settle().await;
    harness.stream(json!({"type": "turn_completed", "turnId": "t-2"}));
    pass_window().await;

    // Assert: the second input's turn ends only with its own completion.
    let tail: Vec<Body> = harness.bodies().into_iter().rev().take(4).collect();
    assert_eq!(
        tail.into_iter().rev().collect::<Vec<_>>(),
        vec![
            Body::InputRejected {
                id: "in-1".to_owned(),
                code: RejectCode::Error,
                reason: Some(super::ADMISSION_FAILURE.to_owned()),
            },
            turn(TurnState::Failed, Some("输入没有送进去")),
            turn(TurnState::Started, None),
            turn(TurnState::Completed, None),
        ]
    );
    assert_eq!(harness.calls("agent.message.send.request").len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_refused_delivery_restarts_the_idle_window() {
    // Arrange
    let mut harness = Harness::idle("claude").await;
    harness
        .host
        .executor
        .always("agent.archive.request", Ok(json!({})));
    tokio::time::sleep(Duration::from_mins(10)).await;
    harness.host.executor.respond(
        "agent.message.send.request",
        Ok(json!({"accepted": false, "error": "Catalog is busy"})),
    );

    // Act
    harness.command(send("in-1", "late"));
    settle().await;
    tokio::time::sleep(Duration::from_mins(14)).await;
    settle().await;
    let still_open = harness.run().status;
    tokio::time::sleep(Duration::from_mins(2)).await;
    harness.until_ended().await;

    // Assert
    assert_eq!(
        still_open,
        RunState::Running,
        "the window restarted at the refusal"
    );
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::ProviderError));
}

#[tokio::test(start_paused = true)]
async fn a_replacement_for_the_current_generation_changes_nothing() {
    // Arrange
    let harness = Harness::live("claude").await;
    harness.timeline(1, assistant("m1", "hello"));
    pass_window().await;
    let before = harness.run();

    // Act: AIT announces the generation the adapter already follows.
    assert!(harness.host.observer.emit(
        AGENT,
        "agent.timeline.replacement",
        json!({"agentId": AGENT, "epoch": AIT_EPOCH})
    ));
    pass_window().await;

    // Assert
    let after = harness.run();
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.next_seq, before.next_seq);
}

#[tokio::test(start_paused = true)]
async fn a_rebuild_shows_our_own_inputs_as_their_original_events() {
    // Arrange: a member input was delivered, then the owner rewound history in AIT.
    let harness = Harness::idle("claude").await;
    harness
        .host
        .executor
        .respond("agent.message.send.request", Ok(json!({"accepted": true})));
    harness.command(send("in-1", "and the tests?"));
    settle().await;
    harness.stream(json!({"type": "turn_completed"}));
    pass_window().await;
    let dispatch_echo = uuid_of(&format!("{RUN}:dispatch"));
    let member_echo = uuid_of("in-1");
    harness.host.backfill.set(
        AGENT,
        Backlog {
            epoch: "ait-2".to_owned(),
            rows: vec![
                Row {
                    seq: 1,
                    provider: "claude".to_owned(),
                    turn_id: None,
                    item: json!({"type": "user_message", "messageId": dispatch_echo,
                                 "clientMessageId": dispatch_echo, "text": "<task …>"}),
                },
                Row {
                    seq: 2,
                    provider: "claude".to_owned(),
                    turn_id: None,
                    item: assistant("m1", "done"),
                },
                Row {
                    seq: 3,
                    provider: "claude".to_owned(),
                    turn_id: None,
                    item: json!({"type": "user_message", "messageId": member_echo,
                                 "clientMessageId": member_echo, "text": "and the tests?"}),
                },
            ],
        },
    );

    // Act
    assert!(harness.host.observer.emit(
        AGENT,
        "agent.timeline.replacement",
        json!({"agentId": AGENT, "epoch": "ait-2"})
    ));
    pass_window().await;

    // Assert: who asked what survives the rebuild, in order.
    assert_eq!(
        harness.bodies(),
        vec![
            dispatch_input(),
            Body::Text {
                mid: "m1".to_owned(),
                text: "done".to_owned()
            },
            member_input("in-1", "and the tests?"),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn polling_a_missing_agent_ends_the_run_as_session_lost() {
    // Arrange
    let mut harness = Harness::idle("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": null, "project": null, "error": null})),
    );

    // Act
    tokio::time::sleep(POLL_INTERVAL + Duration::from_secs(1)).await;
    harness.until_ended().await;

    // Assert
    let run = harness.run();
    assert_eq!(run.status, RunState::Failed);
    assert_eq!(run.reason_code, Some(ReasonCode::SessionLost));
    assert_eq!(harness.bodies().last(), Some(&closed("lost")));
}

fn claude_request(id: &str) -> Value {
    json!({"id": id, "provider": "claude", "name": "Bash", "kind": "tool",
           "input": {"command": "ls"},
           "actions": [{"id": "allow", "behavior": "allow"}, {"id": "deny", "behavior": "deny"}]})
}

#[tokio::test(start_paused = true)]
async fn a_turn_end_lost_with_the_observation_is_settled_from_two_quiet_snapshots() {
    // Arrange: the dispatch turn is open, an input waits, and AIT says the Agent is idle.
    let harness = Harness::live("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "idle", "pendingPermissions": []}})),
    );
    harness.command(Command::Send {
        by: member(),
        input_id: "in-1".to_owned(),
        text: "next".to_owned(),
    });
    settle().await;

    // Act: one poll is not enough.
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;
    let after_one = harness.calls("agent.message.send.request").len();
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;

    // Assert: the second quiet poll ends the turn and the queued input starts the next one.
    assert_eq!(after_one, 0, "one snapshot never ends a turn");
    assert_eq!(harness.calls("agent.message.send.request").len(), 1);
    let bodies = harness.after_start();
    let completed = bodies
        .iter()
        .position(|body| *body == turn(TurnState::Completed, None))
        .expect("the lost turn end is written");
    assert_eq!(bodies[completed + 1], turn(TurnState::Started, None));

    // Act: the new turn is younger than two polls, so the same snapshot leaves it alone.
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;

    // Assert
    assert_eq!(
        harness
            .after_start()
            .iter()
            .filter(|body| **body == turn(TurnState::Completed, None))
            .count(),
        1
    );
}

#[tokio::test(start_paused = true)]
async fn requests_only_the_snapshot_shows_are_added_and_vanished_ones_withdrawn() {
    // Arrange: AIT holds a request whose ID does not fit the contract.
    let harness = Harness::live("claude").await;
    let remapped = crate::translate::ask_id("native id!");
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "running", "pendingPermissions": [claude_request("native id!")]}})),
    );
    let withdrawn = |harness: &Harness, id: &str| {
        harness.bodies().contains(&Body::AskResolved {
            id: id.to_owned(),
            outcome: Outcome::Withdrawn,
            by: None,
            login: None,
        })
    };

    // Act: three polls while AIT still holds it.
    tokio::time::sleep(POLL_INTERVAL * 3 + Duration::from_millis(100)).await;
    settle().await;

    // Assert: added at once (with its record) and never withdrawn while AIT holds it.
    assert!(
        harness
            .bodies()
            .iter()
            .any(|body| matches!(body, Body::Ask { id, .. } if *id == remapped)),
        "{:?}",
        harness.bodies()
    );
    assert_eq!(harness.store.asks(RUN).expect("asks").len(), 1);
    assert!(!withdrawn(&harness, &remapped));

    // Act: an event, then AIT forgets every request.
    harness.stream(json!({"type": "permission_requested", "request": claude_request("perm-2")}));
    settle().await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "running", "pendingPermissions": []}})),
    );
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;
    let after_one = (
        withdrawn(&harness, &remapped),
        withdrawn(&harness, "perm-2"),
    );
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;

    // Assert: one quiet poll after the event is not enough; the second settles both.
    assert_eq!(after_one, (false, false));
    assert!(withdrawn(&harness, &remapped));
    assert!(withdrawn(&harness, "perm-2"));
}

#[tokio::test(start_paused = true)]
async fn the_late_end_of_a_turn_the_snapshot_settled_never_ends_the_next_one() {
    // Arrange: AIT names the turn, then its end event is lost; an input waits.
    let harness = Harness::live("claude").await;
    harness.stream(json!({"type": "turn_started", "turnId": "t-1"}));
    harness.command(Command::Send {
        by: member(),
        input_id: "in-next".to_owned(),
        text: "next".to_owned(),
    });
    settle().await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "idle", "pendingPermissions": []}})),
    );
    tokio::time::sleep(POLL_INTERVAL * 2 + Duration::from_millis(100)).await;
    settle().await;

    // Act: the lost end arrives after all.
    harness.stream(json!({"type": "turn_completed", "turnId": "t-1"}));
    settle().await;
    pass_window().await;

    // Assert: settled once; the input's turn is still open.
    let turns: Vec<Body> = harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::Turn { .. }))
        .collect();
    assert_eq!(
        turns,
        vec![
            turn(TurnState::Started, None),
            turn(TurnState::Completed, None),
            turn(TurnState::Started, None),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_turn_end_waiting_while_the_snapshot_is_fetched_is_handled_first() {
    // Arrange: one quiet poll has passed; an input waits behind the open turn.
    let harness = Harness::live("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "idle", "pendingPermissions": []}})),
    );
    harness.command(Command::Send {
        by: member(),
        input_id: "in-next".to_owned(),
        text: "next".to_owned(),
    });
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;
    let gate = harness.host.executor.hold("agent.get.request");
    tokio::time::sleep(POLL_INTERVAL).await;
    settle().await;

    // Act: the turn ends while the second snapshot is being fetched.
    harness.stream(json!({"type": "turn_completed"}));
    gate.add_permits(1);
    settle().await;
    pass_window().await;

    // Assert: the event ends the turn, and the snapshot does not end the input's turn too.
    let turns: Vec<Body> = harness
        .bodies()
        .into_iter()
        .filter(|body| matches!(body, Body::Turn { .. }))
        .collect();
    assert_eq!(
        turns,
        vec![
            turn(TurnState::Started, None),
            turn(TurnState::Completed, None),
            turn(TurnState::Started, None),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn the_owner_typing_during_an_observation_gap_opens_a_turn() {
    // Arrange: an idle session loses its observation; meanwhile the owner typed in AIT.
    let harness = Harness::idle("claude").await;
    harness.host.backfill.set(
        AGENT,
        Backlog {
            epoch: AIT_EPOCH.to_owned(),
            rows: vec![Row {
                seq: 1,
                provider: "claude".to_owned(),
                turn_id: None,
                item: json!({"type": "user_message", "messageId": "m-gap", "text": "typed in AIT"}),
            }],
        },
    );
    harness.host.observer.close(AGENT);

    // Act
    tokio::time::sleep(Duration::from_secs(1)).await;
    settle().await;
    tokio::time::sleep(IDLE_GRACE + Duration::from_secs(1)).await;
    settle().await;

    // Assert: the backfilled input opened a turn, so idle never archived the Agent under it.
    let tail: Vec<Body> = harness.after_start().into_iter().skip(1).take(2).collect();
    assert_eq!(tail[1], turn(TurnState::Started, None), "{tail:?}");
    assert!(
        matches!(&tail[0], Body::Input { id, .. } if id == "m-gap"),
        "{tail:?}"
    );
    assert_eq!(harness.run().status, RunState::Running);
    assert!(harness.calls("agent.archive.request").is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_cancel_that_arrives_while_the_session_closes_gets_the_final_status() {
    // Arrange: an idle session archiving after a cancel.
    let mut harness = Harness::idle("claude").await;
    let gate = harness.host.executor.hold("agent.archive.request");
    harness.command(Command::Cancel);
    settle().await;

    // Act: the Hub cancels again before the session ended.
    harness.command(Command::Cancel);
    gate.add_permits(1);
    harness.until_ended().await;

    // Assert: both cancels are answered with the final state.
    let statuses = harness.statuses();
    let finals: Vec<RunState> = statuses
        .iter()
        .map(|status| status.status)
        .filter(|state| state.is_terminal())
        .collect();
    assert_eq!(
        finals,
        [RunState::Completed, RunState::Completed],
        "{statuses:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn input_that_arrives_while_the_session_closes_is_still_answered() {
    // Arrange: an idle session archiving after a cancel.
    let mut harness = Harness::idle("claude").await;
    let gate = harness.host.executor.hold("agent.archive.request");
    harness.command(Command::Cancel);
    settle().await;

    // Act
    harness.command(Command::Send {
        by: member(),
        input_id: "in-late".to_owned(),
        text: "one more".to_owned(),
    });
    settle().await;
    gate.add_permits(1);
    harness.until_ended().await;

    // Assert
    assert_eq!(
        harness.bodies().last(),
        Some(&Body::InputRejected {
            id: "in-late".to_owned(),
            code: RejectCode::Closed,
            reason: None,
        })
    );
    assert_eq!(
        input_state(&harness, "in-late").as_deref(),
        Some("rejected")
    );
}

#[tokio::test(start_paused = true)]
async fn the_owner_typing_in_ait_opens_a_turn_the_idle_timer_respects() {
    // Arrange
    let harness = Harness::idle("claude").await;

    // Act
    harness.timeline(
        1,
        json!({"type": "user_message", "messageId": "m-owner", "text": "from the app"}),
    );
    pass_window().await;
    tokio::time::sleep(IDLE_GRACE + Duration::from_secs(1)).await;
    settle().await;

    // Assert: the input opened a turn, so idle never closed the session.
    assert_eq!(
        harness.after_start(),
        vec![
            turn(TurnState::Completed, None),
            Body::Input {
                id: "m-owner".to_owned(),
                text: "from the app".to_owned(),
                by: owner().id,
                login: owner().login,
                origin: Origin::User,
            },
            turn(TurnState::Started, None),
        ]
    );
    assert_eq!(harness.run().status, RunState::Running);
}

#[tokio::test(start_paused = true)]
async fn an_input_is_recorded_as_sent_only_once_ait_took_it() {
    // Arrange
    let harness = Harness::idle("claude").await;
    let gate = harness.host.executor.hold("agent.message.send.request");

    // Act
    harness.command(Command::Send {
        by: member(),
        input_id: "in-2".to_owned(),
        text: "go".to_owned(),
    });
    settle().await;
    let while_sending = input_state(&harness, "in-2");
    gate.add_permits(1);
    settle().await;

    // Assert
    assert_eq!(while_sending.as_deref(), Some("queued"));
    assert_eq!(input_state(&harness, "in-2").as_deref(), Some("sent"));
}

#[tokio::test(start_paused = true)]
async fn an_observation_that_never_opened_is_retried() {
    // Arrange
    let store = Store::memory().expect("open the store");
    let start = start_for(&store, "claude");
    let host = live_host();
    host.observer.fail(2);
    let harness = Harness::launch(host, store, start);
    harness.until_status(RunState::Running).await;

    // Act: two refusals, retried after 250 ms and 500 ms.
    tokio::time::sleep(Duration::from_secs(1)).await;
    settle().await;
    harness.stream(json!({"type": "turn_completed"}));
    settle().await;

    // Assert: the third attempt observes, and its events flow.
    assert_eq!(harness.host.observer.observed(), [AGENT]);
    assert_eq!(
        harness.after_start(),
        vec![turn(TurnState::Completed, None)]
    );
}

#[tokio::test(start_paused = true)]
async fn an_owner_mode_change_keeps_the_runs_own_codex_overrides() {
    // Arrange: full access with approval_policy=never; the owner switches to auto.
    let store = Store::memory().expect("open the store");
    let mut start = start_for(&store, "codex");
    start.applied.mode = "full-access".to_owned();
    start
        .run
        .dispatch
        .settings
        .insert("approval_policy".to_owned(), json!("never"));
    let harness = Harness::launch(live_host(), store, start);
    harness.until_status(RunState::Running).await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"currentModeId": "auto"}})),
    );

    // Act
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;

    // Assert: auto alone would ask; with approval_policy=never it still does not.
    assert_eq!(
        harness.run().execution.map(|execution| execution.approvals),
        Some(false)
    );
    assert!(harness.bodies().iter().any(|body| matches!(body,
        Body::Notice { text, .. } if text.contains("不再停下来等人批"))));
}

#[tokio::test(start_paused = true)]
async fn requests_with_ids_outside_the_pattern_are_answered_by_their_native_id() {
    // Arrange
    let harness = Harness::live("claude").await;
    harness
        .stream(json!({"type": "permission_requested", "request": claude_request("native id!")}));
    settle().await;
    let ask_id = crate::translate::ask_id("native id!");

    // Act
    harness.command(Command::Answer(Box::new(Answer {
        run_id: RUN.to_owned(),
        ask_id: ask_id.clone(),
        option_id: "allow".to_owned(),
        by: member(),
        note: None,
        answers: None,
    })));
    settle().await;
    harness.stream(
        json!({"type": "permission_resolved", "requestId": "native id!",
                          "resolution": {"behavior": "allow"}}),
    );
    settle().await;

    // Assert
    let resolves = harness.calls("agent.permission.resolve.request");
    assert_eq!(resolves[0]["requestId"], "native id!");
    assert!(harness.bodies().contains(&Body::AskResolved {
        id: ask_id,
        outcome: Outcome::Allow,
        by: Some(member().id),
        login: member().login,
    }));
}

#[tokio::test(start_paused = true)]
async fn a_poll_waiting_for_its_slot_never_holds_up_the_session() {
    // Arrange: other sessions spent the shared bucket for about 20 seconds.
    let harness = Harness::idle("claude").await;
    harness.host.executor.always(
        "agent.get.request",
        Ok(json!({"agent": {"status": "idle", "pendingPermissions": []}})),
    );
    {
        let mut bucket = harness.polls.lock().expect("the bucket lock");
        for _ in 0..42 {
            bucket.take();
        }
    }

    // Act: the poll comes due and waits for its slot while the agent keeps talking.
    tokio::time::sleep(POLL_INTERVAL + Duration::from_millis(100)).await;
    settle().await;
    harness.timeline(1, assistant("m-late", "still streaming"));
    pass_window().await;
    let polled_early = harness.calls("agent.get.request").len();
    let streamed_early = harness.bodies().contains(&Body::Text {
        mid: "m-late".to_owned(),
        text: "still streaming".to_owned(),
    });
    // The deficit refills at 2/s: the slot comes about 12 s after the poll fell due.
    tokio::time::sleep(Duration::from_secs(13)).await;
    settle().await;

    // Assert
    assert_eq!(polled_early, 0, "the slot is not there yet");
    assert!(streamed_early, "events flow while the poll waits");
    assert_eq!(
        harness.calls("agent.get.request").len(),
        1,
        "polled once the slot came"
    );
}
