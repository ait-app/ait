//! One run's session: the AIT Agent, its translated event log, input queue and requests.
//!
//! A session task owns everything about its run after `claimed`: it opens the workspace,
//! observes the Agent before creating it, journals neutral events (numbered here, written
//! before they are sent), keeps its own input queue so member input never interrupts a turn,
//! relays approvals, and reports `running` and the final state.

mod buffer;
pub(crate) mod recover;

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::event::{Body, Effect, Event, Level, Origin, Outcome, RejectCode, TurnState, fit};
use crate::outbox::{Outbox, Outgoing};
use crate::ports::{Host, HostEvent, Observation, Workspace};
use crate::prompt;
use crate::store::{AskRecord, InputRecord, RunRecord, StatusUpdate, Store};
use crate::translate::{self, AskSpec, Translator, uuid_of};
use crate::wire::{
    Answer, Execution, Person, REASON_DETAIL_BYTES, ReasonCode, RunState, Status, truncate,
};

use buffer::Buffer;

/// Idle time after which a session is closed and the run completes.
pub const IDLE_GRACE: Duration = Duration::from_mins(15);
/// How long a provider may take to confirm an interrupt during cancellation.
pub const CANCEL_GRACE: Duration = Duration::from_secs(30);
/// How long events gather before they are numbered and sent.
pub const COALESCE: Duration = Duration::from_millis(150);
/// Most queued inputs per run.
pub const QUEUE_LIMIT: usize = 32;
/// How often a live session looks at its Agent snapshot (mode changes, archiving).
pub const POLL_INTERVAL: Duration = Duration::from_secs(8);
/// AIT's `turn_failed` reason when a queued input could not be delivered.
pub const ADMISSION_FAILURE: &str = "Queued input could not be admitted";
/// First wait before re-observing an Agent whose observation closed or never opened.
const REOBSERVE_FIRST: Duration = Duration::from_millis(250);
/// Longest wait between attempts to re-observe.
const REOBSERVE_CAP: Duration = Duration::from_secs(30);
/// Shown once per run when a sub-agent works: its steps are not relayed in v1.
pub const SUBAGENT_NOTICE: &str = "子 agent 的过程不在这里显示,只有它最后交回的结果";

/// Commands from the coordinator.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Command {
    /// Member input.
    Send {
        /// Sender.
        by: Person,
        /// Input ID.
        input_id: String,
        /// Text.
        text: String,
    },
    /// Interrupt the current turn.
    Interrupt,
    /// Answer a request (boxed: it is by far the largest command).
    Answer(Box<Answer>),
    /// Cancel the run.
    Cancel,
    /// The machine owner, learned from `runtime.welcome`.
    Owner(Person),
    /// The runtime was revoked: stop the Agent, close, report nothing.
    Revoke,
    /// The server is stopping: leave the Agent for recovery.
    Shutdown,
    /// Bonsai rejected the frame carrying events `[from, to]` of `epoch` (`4400`).
    Quarantine {
        /// Epoch of the rejected frame.
        epoch: String,
        /// First rejected sequence number.
        from: u64,
        /// Last rejected sequence number.
        to: u64,
    },
}

/// Notifications to the coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    /// The session task finished.
    Ended(String),
}

/// Shared services a session runs on.
#[derive(Debug, Clone)]
pub(crate) struct Context {
    /// Host ports.
    pub(crate) host: Host,
    /// Durable state.
    pub(crate) store: Store,
    /// Current connection.
    pub(crate) outbox: Outbox,
    /// Coordinator notifications.
    pub(crate) notices: mpsc::UnboundedSender<Notice>,
    /// Snapshot polls shared by all sessions (they all use AIT's single worker).
    pub(crate) polls: std::sync::Arc<std::sync::Mutex<crate::outbox::Bucket>>,
}

/// What a new session needs from the coordinator.
#[derive(Debug, Clone)]
pub(crate) struct Start {
    /// The run, already `claimed`.
    pub(crate) run: RunRecord,
    /// Resolved execution.
    pub(crate) execution: Execution,
    /// Host project ID.
    pub(crate) project: String,
    /// Validated settings (mode, thinking, options).
    pub(crate) applied: crate::settings::Applied,
    /// Machine owner, when known.
    pub(crate) owner: Option<Person>,
    /// The dispatch's MCP URL, injected as the session's Bonsai server.
    pub(crate) inject: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum AskState {
    Pending {
        attempt: Option<(Value, Person, Effect)>,
    },
    Resolving {
        by: Person,
        effect: Effect,
    },
    Resolved,
}

#[derive(Debug, Clone)]
struct Ask {
    spec: AskSpec,
    state: AskState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Queued {
    input_id: String,
    message_id: String,
    text: String,
}

/// Spawn a session for a freshly claimed run.
pub(crate) fn spawn(context: Context, start: Start) -> mpsc::UnboundedSender<Command> {
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let run_id = start.run.run_id.clone();
        let notices = context.notices.clone();
        let mut session = Session::new(context, &start);
        let mut receiver = receiver;
        session.run_new(start, &mut receiver).await;
        session.drain(&mut receiver).await;
        let _ = notices.send(Notice::Ended(run_id));
    });
    sender
}

// Independent lifecycle flags of one session task (turn open, closed, finished, …).
#[allow(clippy::struct_excessive_bools)]
struct Session {
    context: Context,
    run_id: String,
    agent_id: String,
    space_id: String,
    execution: Execution,
    translator: Translator,
    epoch: String,
    next_seq: u64,
    ait_cursor: Option<(String, u64)>,
    buffer: Buffer,
    flush_at: Option<Instant>,
    turn_open: bool,
    queue: VecDeque<Queued>,
    asks: HashMap<String, Ask>,
    closed: bool,
    last_turn_failed: bool,
    messages: Messages,
    idle_since: Instant,
    cancel_deadline: Option<Instant>,
    observation: Option<Observation>,
    /// When to try observing again while there is no observation.
    reobserve_at: Option<Instant>,
    reobserve_backoff: Duration,
    finished: bool,
    subagent_noted: bool,
    mode: Option<String>,
    /// The dispatch's settings, for recomputing `approvals` when the owner changes the mode.
    settings: serde_json::Map<String, Value>,
    poll_at: Option<Instant>,
    /// A poll slot was already taken from the shared bucket for `poll_at`.
    poll_reserved: bool,
    /// Polls in a row with no Agent event in between; the snapshot is trusted for turns and
    /// requests only from the second one on.
    quiet_polls: u32,
    /// AIT's ID of the turn in progress, from `turn_started` / `usage_updated`.
    ait_turn: Option<String>,
    /// A turn the snapshot settled: its own late end event must not end a newer turn.
    settled_turn: Option<String>,
    /// AIT announces a refused delivery with a `turn_failed` that has no turn ID; one is owed
    /// per refused send and must not end a later turn.
    refused_deliveries: usize,
}

impl Session {
    fn new(context: Context, start: &Start) -> Self {
        let hex = start.run.run_id.trim_start_matches("r_");
        Self {
            run_id: start.run.run_id.clone(),
            agent_id: uuid_of(hex),
            space_id: start.run.dispatch.space_id.clone(),
            execution: start.execution.clone(),
            translator: Translator::new(start.owner.clone()),
            epoch: start.run.epoch.clone(),
            next_seq: start.run.next_seq,
            ait_cursor: start.run.ait_cursor.clone(),
            buffer: Buffer::default(),
            flush_at: None,
            turn_open: false,
            queue: VecDeque::new(),
            asks: HashMap::new(),
            closed: false,
            last_turn_failed: false,
            messages: Messages::default(),
            idle_since: Instant::now(),
            cancel_deadline: None,
            observation: None,
            reobserve_at: None,
            reobserve_backoff: REOBSERVE_FIRST,
            finished: false,
            subagent_noted: false,
            mode: Some(start.applied.mode.clone()),
            settings: start.run.dispatch.settings.clone(),
            poll_at: None,
            poll_reserved: false,
            quiet_polls: 0,
            ait_turn: None,
            settled_turn: None,
            refused_deliveries: 0,
            context,
        }
    }

    async fn run_new(&mut self, start: Start, commands: &mut mpsc::UnboundedReceiver<Command>) {
        if self.create(&start).await {
            self.serve(commands).await;
        }
        self.flush();
    }

    /// Answer what reached the session after it stopped serving: every `session.send` gets an
    /// `input` or `input_rejected`, every cancel a status (protocol §4.4, §5).
    async fn drain(&mut self, commands: &mut mpsc::UnboundedReceiver<Command>) {
        commands.close();
        while let Ok(command) = commands.try_recv() {
            match command {
                // Closed: rejected. Shut down: kept as queued for recovery to deliver.
                Command::Send { by, input_id, text } => {
                    self.receive_input(by, input_id, text).await;
                }
                Command::Cancel if self.closed => self.resend_status(),
                Command::Quarantine { epoch, from, to } => self.quarantine_range(&epoch, from, to),
                _ => {}
            }
        }
        self.flush();
    }

    /// Send the stored status again (a cancel arrived after the run ended).
    fn resend_status(&self) {
        let Ok(Some(run)) = self.context.store.run(&self.run_id) else {
            return;
        };
        let failed = run.status == RunState::Failed;
        self.context.outbox.send(Outgoing::Status(Status {
            kind: "run.status",
            run_id: run.run_id,
            status: run.status,
            at: run.status_at,
            execution: run.execution,
            reason_code: run.reason_code.filter(|_| failed),
            reason_detail: run.reason_detail.filter(|_| failed),
            final_text: run.final_text,
        }));
    }

    /// Open the workspace, observe the Agent, journal the dispatch and create the Agent.
    ///
    /// Returns whether a live session now exists.
    async fn create(&mut self, start: &Start) -> bool {
        let workspace = match self
            .context
            .host
            .projects
            .open_workspace(&start.project)
            .await
        {
            Ok(Some(workspace)) => workspace,
            Ok(None) => {
                self.fail(ReasonCode::ProjectUnavailable, "项目目录不在了或项目已归档");
                return false;
            }
            Err(_) => {
                self.fail(ReasonCode::ProjectUnavailable, "读取项目登记失败");
                return false;
            }
        };
        let activated = self
            .context
            .host
            .observer
            .observe(&self.agent_id)
            .ok()
            .and_then(|mut observation| observation.activate().is_ok().then_some(observation));
        if activated.is_none() {
            tracing::warn!(
                run_id = self.run_id,
                "agent events are unavailable; retrying"
            );
            self.retry_observe();
        }
        self.observation = activated;
        let dispatch = &start.run.dispatch;
        self.push(Body::Input {
            id: format!("dispatch-{}", dispatch_suffix(&self.run_id)),
            text: prompt::dispatch_input(dispatch),
            by: dispatch.requested_by.id.clone(),
            login: dispatch.requested_by.login.clone(),
            origin: Origin::Dispatch,
        });
        self.push(Body::Turn {
            state: TurnState::Started,
            reason: None,
        });
        self.turn_open = true;
        self.flush();
        if self.cancel_requested() {
            self.close("cancelled");
            self.report(RunState::Cancelled, None, None);
            return false;
        }
        let client_message_id = uuid_of(&format!("{}:dispatch", self.run_id));
        self.translator.sent(&client_message_id);
        let params = self.create_params(start, &workspace, &client_message_id);
        let Some(params) = params else {
            self.push(Body::Turn {
                state: TurnState::Failed,
                reason: Some("system prompt 超过 64 KiB".to_owned()),
            });
            self.turn_open = false;
            self.close("error");
            self.report(
                RunState::Failed,
                Some(ReasonCode::Rejected),
                Some("system prompt 超过 64 KiB".to_owned()),
            );
            return false;
        };
        let created = self
            .context
            .host
            .executor
            .execute("agent.create.request", params)
            .await;
        self.created(created).await
    }

    /// Record the outcome of `agent.create`; returns whether a live session now exists.
    async fn created(&mut self, created: Result<Value, model::ErrorCode>) -> bool {
        match created {
            Ok(value) if value["error"].is_null() && value["agentId"].is_string() => {
                if let Err(error) = self.context.store.set_agent(&self.run_id, &self.agent_id) {
                    tracing::error!(?error, "recording the agent failed");
                }
                if let Some(model) = value["agent"]["runtimeInfo"]["model"]
                    .as_str()
                    .filter(|model| crate::wire::is_model_id(model))
                {
                    self.execution.model = Some(model.to_owned());
                    let _ = self
                        .context
                        .store
                        .set_execution(&self.run_id, &self.execution);
                }
                self.report(RunState::Running, None, None);
                if self.cancel_requested() {
                    self.cancel().await;
                }
                true
            }
            other => {
                let detail = self.creation_failure(&other).await;
                self.push(Body::Turn {
                    state: TurnState::Failed,
                    reason: Some(detail.1.clone()),
                });
                self.turn_open = false;
                self.close("error");
                if self.cancel_requested() {
                    self.report(RunState::Cancelled, None, None);
                } else {
                    self.report(RunState::Failed, Some(detail.0), Some(detail.1));
                }
                false
            }
        }
    }

    fn create_params(
        &self,
        start: &Start,
        workspace: &Workspace,
        client_message_id: &str,
    ) -> Option<Value> {
        let dispatch = &start.run.dispatch;
        let applied = &start.applied;
        let system_prompt = prompt::system_prompt(
            &dispatch.wrapup,
            self.execution.bonsai_write,
            applied.append_system_prompt.as_deref(),
        )
        .ok()?;
        let mut config = json!({
            "provider": self.execution.provider,
            "cwd": workspace.cwd,
            "title": prompt::title(&dispatch.task.text),
            "modeId": applied.mode,
            "systemPrompt": system_prompt,
        });
        if let Some(model) = &dispatch.model {
            config["model"] = Value::from(model.clone());
        }
        if let Some(thinking) = &applied.thinking {
            config["thinkingOptionId"] = Value::from(thinking.clone());
        }
        if let Some(fast) = applied.fast {
            config["featureValues"] = json!({"fast_mode": fast});
        }
        // Only this session's MCP servers load: never the owner's own Bonsai connectors.
        let mut options = applied.provider_options.clone();
        options.insert("strictMcp".to_owned(), Value::Bool(true));
        config["providerOptions"] = Value::Object(options);
        if let Some(url) = &start.inject {
            config["mcpServers"] =
                json!({ crate::settings::BONSAI_SERVER: {"type": "http", "url": url} });
            if applied.preapprove_bonsai {
                config["toolPolicy"] = crate::settings::bonsai_tool_policy();
            }
        }
        Some(json!({
            "agentId": self.agent_id,
            "idempotencyKey": self.run_id,
            "workspaceId": workspace.workspace_id,
            "labels": {"bonsai.run": self.run_id, "bonsai.space": self.space_id},
            "config": config,
            "initialPrompt": prompt::user_turn(dispatch),
            "clientMessageId": client_message_id,
        }))
    }

    async fn creation_failure(
        &self,
        result: &Result<Value, model::ErrorCode>,
    ) -> (ReasonCode, String) {
        match result {
            Err(model::ErrorCode::UnsupportedCapability) => {
                let available = self
                    .context
                    .host
                    .executor
                    .execute("provider.available.list.request", json!({}))
                    .await
                    .ok()
                    .and_then(|value| {
                        value["providers"].as_array().map(|providers| {
                            providers.iter().any(|entry| {
                                entry["provider"] == self.execution.provider.as_str()
                                    && entry["available"] == true
                            })
                        })
                    });
                if available == Some(false) {
                    (
                        ReasonCode::ProviderUnavailable,
                        format!("{} 在这台机器上不可用", self.execution.provider),
                    )
                } else {
                    (
                        ReasonCode::ProviderError,
                        "AIT 拒绝建会话:多半是同时存活的会话满了(上限 32,和机器主人自己在 AIT 里开的会话共用),也可能是 provider 不收这组设定".to_owned(),
                    )
                }
            }
            Err(code) => (
                ReasonCode::ProviderError,
                format!("AIT 建会话失败:{}", code.message()),
            ),
            Ok(value) => (
                ReasonCode::ProviderError,
                format!(
                    "AIT 建会话失败:{}",
                    value["error"].as_str().unwrap_or("没有返回会话")
                ),
            ),
        }
    }

    async fn serve(&mut self, commands: &mut mpsc::UnboundedReceiver<Command>) {
        self.idle_since = Instant::now();
        self.poll_at = Some(Instant::now() + POLL_INTERVAL);
        while !self.finished {
            let idle_at = (!self.turn_open && !self.closed).then(|| self.idle_since + IDLE_GRACE);
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.command(command).await,
                    None => return,
                },
                event = next_event(self.observation.as_mut()) => match event {
                    Some(event) => self.event(event).await,
                    None => self.observation_lost(),
                },
                () = sleep_until(self.reobserve_at) => self.reobserve().await,
                () = sleep_until(self.flush_at) => self.flush(),
                () = sleep_until(idle_at) => self.idle().await,
                () = sleep_until(self.cancel_deadline) => self.force_stop().await,
                () = sleep_until(self.poll_at) => self.poll().await,
            }
        }
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::Send { by, input_id, text } => self.receive_input(by, input_id, text).await,
            Command::Interrupt => {
                if self.turn_open && !self.closed {
                    self.interrupt().await;
                }
            }
            Command::Answer(answer) => self.answer(*answer).await,
            Command::Cancel => self.cancel().await,
            Command::Owner(owner) => self.translator.set_owner(owner),
            Command::Revoke => {
                if self.turn_open {
                    self.interrupt().await;
                }
                self.archive().await;
                self.close("revoked");
                self.finished = true;
            }
            Command::Shutdown => {
                self.flush();
                self.finished = true;
            }
            Command::Quarantine { epoch, from, to } => self.quarantine_range(&epoch, from, to),
        }
    }

    /// Look at the Agent snapshot: AIT publishes no event when the owner changes the mode or
    /// archives the Agent in the AIT app.
    async fn poll(&mut self) {
        self.poll_at = None;
        if self.closed {
            return;
        }
        if !std::mem::take(&mut self.poll_reserved) {
            // Reserve a slot in the shared bucket without waiting here: the session keeps
            // serving events, commands and timers until the slot comes.
            let wait = self
                .context
                .polls
                .lock()
                .map(|mut bucket| {
                    let wait = bucket.wait();
                    bucket.take();
                    wait
                })
                .unwrap_or_default();
            if !wait.is_zero() {
                self.poll_reserved = true;
                self.poll_at = Some(Instant::now() + wait);
                return;
            }
        }
        let snapshot = self
            .context
            .host
            .executor
            .execute("agent.get.request", json!({"agentId": self.agent_id}))
            .await;
        let delay = match snapshot {
            Ok(snapshot) if snapshot["agent"].is_object() => {
                // Events AIT published before answering are older than the snapshot: handle
                // them first, so a turn end waiting in the channel is never doubled by it.
                self.take_ready_events().await;
                if !self.finished {
                    self.observe_snapshot(&snapshot["agent"]).await;
                }
                POLL_INTERVAL
            }
            Ok(snapshot) if snapshot["agent"].is_null() && snapshot.get("agent").is_some() => {
                self.observation = None;
                self.close("lost");
                self.report(
                    RunState::Failed,
                    Some(ReasonCode::SessionLost),
                    Some("执行端上已经没有这个会话了".to_owned()),
                );
                self.finished = true;
                return;
            }
            Err(model::ErrorCode::CatalogBusy) => POLL_INTERVAL * 2,
            _ => POLL_INTERVAL,
        };
        if !self.finished {
            self.poll_at = Some(Instant::now() + delay);
        }
    }

    async fn observe_snapshot(&mut self, agent: &Value) {
        if agent["archivedAt"].is_string() {
            self.observation = None;
            self.close("archived");
            if self.last_turn_failed {
                self.report(
                    RunState::Failed,
                    Some(ReasonCode::ProviderError),
                    Some("机器主人在本机归档了这个会话,最后一轮是失败的".to_owned()),
                );
            } else {
                self.report(RunState::Completed, None, None);
            }
            self.finished = true;
            return;
        }
        self.reconcile(agent).await;
        if self.finished {
            return;
        }
        let mode = agent["currentModeId"].as_str().map(str::to_owned);
        if mode.is_some() && mode != self.mode {
            let new_mode = mode.clone().unwrap_or_default();
            self.mode = mode;
            let _ = self
                .context
                .store
                .set_mode(&self.run_id, self.mode.as_deref());
            let approvals = crate::settings::mode_asks_with(
                &self.execution.provider,
                &new_mode,
                &self.settings,
            );
            self.execution.approvals = approvals;
            let _ = self
                .context
                .store
                .set_execution(&self.run_id, &self.execution);
            let consequence = if approvals {
                "工具调用会停下来等人批"
            } else {
                "工具调用不再停下来等人批"
            };
            self.push(Body::Notice {
                level: Level::Warning,
                text: format!("机器主人在本机把权限模式改成了 {new_mode}:从下一轮起{consequence}"),
            });
            self.flush();
        }
    }

    /// Turns and requests have no timeline rows, so events lost while the observation was
    /// closed are only visible in the snapshot (adapter §3.4). Requests it shows are added at
    /// once; a turn or request it no longer shows is settled only after two polls in a row with
    /// no Agent event in between, so a turn that just started is never ended.
    async fn reconcile(&mut self, agent: &Value) {
        let pending: Vec<Value> = agent["pendingPermissions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let had_asks = self.asks.len();
        for request in &pending {
            self.requested(request);
        }
        if self.asks.len() != had_asks {
            self.quiet_polls = 0;
        }
        self.quiet_polls = self.quiet_polls.saturating_add(1);
        if !self.turn_open && agent["status"] == "running" {
            // A turn this log has not seen started (the owner, during an observation gap):
            // never archive the Agent under it.
            self.idle_since = Instant::now();
        }
        if self.quiet_polls < 2 || agent.get("status").is_none() {
            return;
        }
        let live: Vec<&str> = pending
            .iter()
            .filter_map(|request| request["id"].as_str())
            .collect();
        let vanished: Vec<String> = self
            .asks
            .iter()
            .filter(|(_, ask)| matches!(ask.state, AskState::Pending { .. }))
            .filter(|(_, ask)| !live.contains(&ask.spec.native_id()))
            .map(|(id, _)| id.clone())
            .collect();
        for id in vanished {
            self.settle(&id, Outcome::Withdrawn, None);
        }
        if self.turn_open && self.cancel_deadline.is_none() && agent["status"] != "running" {
            self.settled_turn = self.ait_turn.take();
            match agent["status"].as_str() {
                Some("idle") => self.end_turn(TurnState::Completed, None).await,
                Some("error") => {
                    let reason = agent["lastError"]
                        .as_str()
                        .unwrap_or("执行端报错")
                        .to_owned();
                    self.end_turn(TurnState::Failed, Some(reason)).await;
                }
                Some("closed") => {
                    self.end_turn(TurnState::Aborted, Some("session_closed".to_owned()))
                        .await;
                }
                _ => {}
            }
        }
        self.flush();
    }

    async fn event(&mut self, event: HostEvent) {
        self.quiet_polls = 0;
        self.reobserve_backoff = REOBSERVE_FIRST;
        match event.method.as_str() {
            "agent_stream" => self.stream(&event.params).await,
            "agent.timeline.replacement" => {
                let current = self.ait_cursor.as_ref().map(|(epoch, _)| epoch.as_str());
                if event.params["epoch"].as_str() != current {
                    self.rebuild().await;
                }
            }
            // Sub-agent rows carry the child's own seq/epoch: never mix them into the parent's.
            "agent.provider_subagents.update" if !self.subagent_noted => {
                self.subagent_noted = true;
                self.push(Body::Notice {
                    level: Level::Info,
                    text: SUBAGENT_NOTICE.to_owned(),
                });
            }
            _ => {}
        }
        if self.flush_at.is_none() && !self.buffer.is_empty() {
            self.flush_at = Some(Instant::now() + COALESCE);
        }
    }

    async fn stream(&mut self, params: &Value) {
        let event = &params["event"];
        let provider = event["provider"]
            .as_str()
            .unwrap_or(&self.execution.provider)
            .to_owned();
        match event["type"].as_str() {
            Some("timeline") => {
                let (Some(epoch), Some(seq)) = (params["epoch"].as_str(), params["seq"].as_u64())
                else {
                    return;
                };
                if let Some((cursor_epoch, cursor_seq)) = &self.ait_cursor {
                    if cursor_epoch == epoch && seq <= *cursor_seq {
                        return;
                    }
                    if cursor_epoch != epoch {
                        self.rebuild().await;
                        return;
                    }
                }
                self.ait_cursor = Some((epoch.to_owned(), seq));
                for body in self.translator.item(&provider, &event["item"]) {
                    self.push_row(body);
                }
            }
            Some("turn_completed" | "turn_failed" | "turn_canceled") if self.is_settled(event) => {}
            Some("turn_started") => {
                self.ait_turn = event["turnId"].as_str().map(str::to_owned);
                if !self.turn_open && !self.closed {
                    self.push(Body::Turn {
                        state: TurnState::Started,
                        reason: None,
                    });
                    self.turn_open = true;
                }
            }
            Some("usage_updated") => {
                if let Some(turn) = event["turnId"].as_str() {
                    self.ait_turn = Some(turn.to_owned());
                }
                if let Some(body) = translate::usage(&event["usage"]) {
                    self.push(body);
                }
            }
            Some("turn_completed") => {
                if let Some(body) = translate::usage(&event["usage"]) {
                    self.push(body);
                }
                self.end_turn(TurnState::Completed, None).await;
            }
            Some("turn_failed") => {
                let reason = event["error"].as_str().map(str::to_owned);
                let admission = event.get("turnId").is_none_or(Value::is_null)
                    && reason.as_deref() == Some(ADMISSION_FAILURE);
                if admission && self.refused_deliveries > 0 {
                    self.refused_deliveries -= 1;
                    return;
                }
                self.end_turn(TurnState::Failed, reason).await;
            }
            Some("turn_canceled") => {
                let reason = event["reason"].as_str().unwrap_or("interrupted").to_owned();
                self.end_turn(TurnState::Aborted, Some(reason)).await;
            }
            Some("permission_requested") => self.requested(&event["request"]),
            Some("permission_resolved") => {
                if let Some(native) = event["requestId"].as_str() {
                    self.resolved(&translate::ask_id(native), &event["resolution"]);
                }
            }
            _ => {}
        }
    }

    /// Push a translated timeline row. Only the owner's own typing in AIT survives translation
    /// as input, and AIT announces no turn start for it: it opens one here.
    fn push_row(&mut self, body: Body) {
        let owner_turn = matches!(body, Body::Input { .. }) && !self.turn_open && !self.closed;
        self.push(body);
        if owner_turn {
            self.push(Body::Turn {
                state: TurnState::Started,
                reason: None,
            });
            self.turn_open = true;
        }
    }

    /// Whether a turn end belongs to the turn the snapshot already settled.
    fn is_settled(&mut self, event: &Value) -> bool {
        let settled =
            self.settled_turn.is_some() && event["turnId"].as_str() == self.settled_turn.as_deref();
        if settled {
            self.settled_turn = None;
        }
        settled
    }

    /// Handle the events already waiting in the observation, without waiting for more.
    async fn take_ready_events(&mut self) {
        use futures_util::FutureExt;
        loop {
            let Some(observation) = self.observation.as_mut() else {
                return;
            };
            match observation.next().now_or_never() {
                Some(Some(event)) => self.event(event).await,
                Some(None) => {
                    self.observation_lost();
                    return;
                }
                None => return,
            }
        }
    }

    /// Replace a range Bonsai rejected (`4400`) with one notice in a new epoch.
    fn quarantine_range(&mut self, epoch: &str, from: u64, to: u64) {
        self.flush();
        if let Some((epoch, next)) = quarantine(&self.context.store, &self.run_id, epoch, from, to)
        {
            self.epoch = epoch;
            self.next_seq = next;
        }
    }

    fn requested(&mut self, request: &Value) {
        let Some(spec) = self.translator.ask(request) else {
            return;
        };
        if self.asks.contains_key(&spec.id) {
            return;
        }
        self.persist_ask(&spec, &AskState::Pending { attempt: None });
        self.push(spec.body());
        self.asks.insert(
            spec.id.clone(),
            Ask {
                spec,
                state: AskState::Pending { attempt: None },
            },
        );
        // Journal the request with its record, so a crash cannot leave one without the other.
        self.flush();
    }

    fn resolved(&mut self, id: &str, resolution: &Value) {
        let Some(ask) = self.asks.get(id).cloned() else {
            return;
        };
        let (outcome, by) = match &ask.state {
            AskState::Resolved => return,
            AskState::Resolving { by, effect } => (outcome_of(*effect), Some(by.clone())),
            AskState::Pending { attempt } => match attempt {
                Some((response, by, effect)) if response == resolution => {
                    (outcome_of(*effect), Some(by.clone()))
                }
                _ if translate::is_withdrawal(resolution) => (Outcome::Withdrawn, None),
                _ => (
                    outcome_of(translate::effect_of(resolution)),
                    Some(self.owner()),
                ),
            },
        };
        self.settle(id, outcome, by);
        self.flush();
    }

    fn settle(&mut self, id: &str, outcome: Outcome, by: Option<Person>) {
        let Some(ask) = self.asks.get_mut(id) else {
            return;
        };
        ask.state = AskState::Resolved;
        let spec = ask.spec.clone();
        self.persist_ask(&spec, &AskState::Resolved);
        self.push(Body::AskResolved {
            id: id.to_owned(),
            outcome,
            by: by.as_ref().map(|person| person.id.clone()),
            login: by.and_then(|person| person.login),
        });
    }

    /// Settle every open request: confirmed answers keep their effect, the rest are withdrawn.
    fn withdraw_open_asks(&mut self) {
        let open: Vec<(String, AskState)> = self
            .asks
            .iter()
            .filter(|(_, ask)| ask.state != AskState::Resolved)
            .map(|(id, ask)| (id.clone(), ask.state.clone()))
            .collect();
        for (id, state) in open {
            match state {
                AskState::Resolving { by, effect } => {
                    self.settle(&id, outcome_of(effect), Some(by));
                }
                _ => self.settle(&id, Outcome::Withdrawn, None),
            }
        }
    }

    async fn answer(&mut self, answer: Answer) {
        let Some(ask) = self.asks.get(&answer.ask_id).cloned() else {
            return;
        };
        if !matches!(ask.state, AskState::Pending { .. }) || self.closed {
            return;
        }
        let (response, effect) = match ask.spec.response(&answer) {
            Ok(built) => built,
            Err(reason) => {
                self.push(Body::Notice {
                    level: Level::Error,
                    text: format!("这个回答用不了:{reason}"),
                });
                self.flush();
                return;
            }
        };
        let resolving = AskState::Resolving {
            by: answer.by.clone(),
            effect,
        };
        self.persist_ask(&ask.spec, &resolving);
        if let Some(entry) = self.asks.get_mut(&answer.ask_id) {
            entry.state = resolving;
        }
        let result = crate::hello::execute_retrying(
            self.context.host.executor.as_ref(),
            "agent.permission.resolve.request",
            json!({"agentId": self.agent_id, "requestId": ask.spec.native_id(), "response": response}),
        )
        .await;
        if result.is_err() {
            let pending = AskState::Pending {
                attempt: Some((response, answer.by.clone(), effect)),
            };
            self.persist_ask(&ask.spec, &pending);
            if let Some(entry) = self.asks.get_mut(&answer.ask_id) {
                entry.state = pending;
            }
            self.push(Body::Notice {
                level: Level::Error,
                text: "回答没有送到执行端,请再试一次".to_owned(),
            });
            self.flush();
        }
    }

    async fn receive_input(&mut self, by: Person, input_id: String, text: String) {
        let message_id = uuid_of(&input_id);
        let state = if self.closed { "rejected" } else { "queued" };
        let record = InputRecord {
            input_id: input_id.clone(),
            message_id: message_id.clone(),
            text: text.clone(),
            state: state.to_owned(),
            by: Some(by.id.clone()),
            login: by.login.clone(),
        };
        match self.context.store.insert_input(&self.run_id, &record) {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::error!(?error, "recording an input failed");
                return;
            }
        }
        if self.closed {
            self.push(Body::InputRejected {
                id: input_id,
                code: RejectCode::Closed,
                reason: None,
            });
            self.flush();
            return;
        }
        self.push(Body::Input {
            id: input_id.clone(),
            text: text.clone(),
            by: by.id,
            login: by.login,
            origin: Origin::User,
        });
        if self.queue.len() >= QUEUE_LIMIT {
            let _ = self
                .context
                .store
                .set_input_state(&self.run_id, &input_id, "rejected");
            self.push(Body::InputRejected {
                id: input_id,
                code: RejectCode::Busy,
                reason: Some("排队的消息太多了".to_owned()),
            });
            self.flush();
            return;
        }
        self.queue.push_back(Queued {
            input_id,
            message_id,
            text,
        });
        self.idle_since = Instant::now();
        if self.turn_open {
            self.flush();
        } else {
            self.deliver_next().await;
        }
    }

    /// Start the next queued input as a new turn; failed deliveries move on to the next one.
    async fn deliver_next(&mut self) {
        while !self.turn_open && !self.closed && self.cancel_deadline.is_none() {
            let Some(queued) = self.queue.pop_front() else {
                self.flush();
                return;
            };
            self.push(Body::Turn {
                state: TurnState::Started,
                reason: None,
            });
            self.turn_open = true;
            // A snapshot taken before AIT starts this turn must not end it.
            self.quiet_polls = 0;
            self.flush();
            self.translator.sent(&queued.message_id);
            // Retried only while AIT's worker is busy; a repeated messageId is idempotent.
            let result = crate::hello::execute_retrying(
                self.context.host.executor.as_ref(),
                "agent.message.send.request",
                json!({"agentId": self.agent_id, "text": queued.text,
                       "messageId": queued.message_id, "activeTurnBehavior": "steer"}),
            )
            .await;
            let failure = match &result {
                Ok(value) if value["accepted"] == true => None,
                Ok(value) => Some(
                    value["error"]
                        .as_str()
                        .unwrap_or("执行端没有收下")
                        .to_owned(),
                ),
                Err(code) => Some(code.message().to_owned()),
            };
            // Recorded as sent only once AIT took it: an input lost in a crash before that is
            // still queued after the restart and delivered again under the same messageId.
            if failure.is_none() {
                let _ = self
                    .context
                    .store
                    .set_input_state(&self.run_id, &queued.input_id, "sent");
            }
            if let Some(reason) = failure {
                let _ =
                    self.context
                        .store
                        .set_input_state(&self.run_id, &queued.input_id, "rejected");
                let (reason, _) = truncate(&reason, 512);
                self.push(Body::InputRejected {
                    id: queued.input_id,
                    code: RejectCode::Error,
                    reason: Some(reason.to_owned()),
                });
                self.push(Body::Turn {
                    state: TurnState::Failed,
                    reason: Some("输入没有送进去".to_owned()),
                });
                self.turn_open = false;
                self.last_turn_failed = true;
                self.idle_since = Instant::now();
                if result.is_ok() {
                    self.refused_deliveries += 1;
                }
                self.flush();
            }
        }
    }

    async fn end_turn(&mut self, state: TurnState, reason: Option<String>) {
        if !self.turn_open {
            return;
        }
        self.withdraw_open_asks();
        self.push(Body::Turn { state, reason });
        self.turn_open = false;
        self.last_turn_failed = state == TurnState::Failed;
        self.idle_since = Instant::now();
        self.flush();
        if self.cancel_deadline.is_some() {
            self.finish_cancel().await;
            return;
        }
        self.deliver_next().await;
    }

    async fn interrupt(&mut self) {
        if let Err(code) = crate::hello::execute_retrying(
            self.context.host.executor.as_ref(),
            "agent.cancel.request",
            json!({"agentId": self.agent_id}),
        )
        .await
        {
            tracing::warn!(
                run_id = self.run_id,
                code = code.message(),
                "interrupt failed"
            );
        }
    }

    /// Cancel per protocol §4.4: interrupt and confirm a running turn, close an idle session.
    async fn cancel(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.context.store.request_cancel(&self.run_id);
        if self.turn_open {
            if self.cancel_deadline.is_none() {
                self.cancel_deadline = Some(Instant::now() + CANCEL_GRACE);
                self.interrupt().await;
            }
            return;
        }
        self.archive().await;
        self.close("cancelled");
        self.report(RunState::Completed, None, None);
        self.finished = true;
    }

    async fn finish_cancel(&mut self) {
        self.cancel_deadline = None;
        self.archive().await;
        self.close("cancelled");
        self.report(RunState::Cancelled, None, None);
        self.finished = true;
    }

    /// The provider did not confirm the interrupt in time: archiving kills its process group.
    async fn force_stop(&mut self) {
        if self.turn_open {
            self.withdraw_open_asks();
            self.push(Body::Turn {
                state: TurnState::Aborted,
                reason: Some("cancelled".to_owned()),
            });
            self.turn_open = false;
        }
        self.finish_cancel().await;
    }

    async fn idle(&mut self) {
        self.archive().await;
        self.close("idle");
        if self.last_turn_failed {
            self.report(
                RunState::Failed,
                Some(ReasonCode::ProviderError),
                Some("最后一轮失败之后没有新的输入".to_owned()),
            );
        } else {
            self.report(RunState::Completed, None, None);
        }
        self.finished = true;
    }

    async fn archive(&mut self) {
        if let Err(code) = crate::hello::execute_retrying(
            self.context.host.executor.as_ref(),
            "agent.archive.request",
            json!({"agentId": self.agent_id}),
        )
        .await
        {
            tracing::error!(
                run_id = self.run_id,
                code = code.message(),
                "archive failed; the agent stays in AIT"
            );
        }
        self.observation = None;
        self.reobserve_at = None;
    }

    /// The host closed the observation: try again after a backoff (a host that keeps closing
    /// observations, such as one shutting down, must not spin this task).
    fn observation_lost(&mut self) {
        self.observation = None;
        // Events may have been dropped: the next snapshots start counting quiet polls anew.
        self.quiet_polls = 0;
        if !self.closed {
            self.retry_observe();
        }
    }

    /// Re-observe, backfilling what live delivery missed; the next poll reconciles turns and
    /// requests, which have no timeline rows.
    async fn reobserve(&mut self) {
        self.reobserve_at = None;
        if self.closed || self.observation.is_some() {
            return;
        }
        let Ok(mut observation) = self.context.host.observer.observe(&self.agent_id) else {
            self.retry_observe();
            return;
        };
        self.backfill_with(false).await;
        if observation.activate().is_ok() {
            self.observation = Some(observation);
            if !self.poll_reserved {
                self.poll_at = Some(Instant::now());
            }
        } else {
            self.retry_observe();
        }
    }

    fn retry_observe(&mut self) {
        self.reobserve_at = Some(Instant::now() + self.reobserve_backoff);
        self.reobserve_backoff = (self.reobserve_backoff * 2).min(REOBSERVE_CAP);
    }

    /// Translate rows live delivery missed. When AIT started a new generation: after a restart
    /// (`reanchor`) its reconciliation only re-reads native history, so the log we already sent
    /// stays valid and the cursor moves to the new generation's end; otherwise history was
    /// rewritten and the log is rebuilt.
    async fn backfill_with(&mut self, reanchor: bool) {
        let Ok(backlog) = self.context.host.backfill.read(&self.agent_id).await else {
            return;
        };
        let same = self
            .ait_cursor
            .as_ref()
            .is_none_or(|(epoch, _)| *epoch == backlog.epoch);
        if !same && reanchor {
            let end = backlog.rows.last().map_or(0, |row| row.seq);
            self.ait_cursor = Some((backlog.epoch.clone(), end));
            self.flush();
            let cursor = self
                .ait_cursor
                .as_ref()
                .map(|(epoch, seq)| (epoch.as_str(), *seq));
            let _ =
                self.context
                    .store
                    .append(&self.run_id, &self.epoch, self.next_seq, &[], cursor);
            return;
        }
        if !same {
            self.rebuild().await;
            return;
        }
        let after = self.ait_cursor.as_ref().map(|(_, seq)| *seq);
        for row in backlog.rows {
            if after.is_some_and(|after| row.seq <= after) {
                continue;
            }
            // Rows live delivery missed are Agent activity like any event.
            self.quiet_polls = 0;
            self.ait_cursor = Some((backlog.epoch.clone(), row.seq));
            for body in self.translator.item(&row.provider, &row.item) {
                // After a restart the turn state of missed rows is unknown; recovery settles
                // any open turn itself.
                if reanchor {
                    self.push(body);
                } else {
                    self.push_row(body);
                }
            }
        }
        self.flush();
    }

    /// AIT rewrote the timeline: start a new epoch and rebuild the log from its rows.
    async fn rebuild(&mut self) {
        let Ok(backlog) = self.context.host.backfill.read(&self.agent_id).await else {
            return;
        };
        self.buffer.take();
        self.flush_at = None;
        let epoch = new_epoch();
        if let Err(error) = self.context.store.rotate(&self.run_id, &epoch) {
            tracing::error!(?error, "rotating the run log failed");
            return;
        }
        self.epoch = epoch;
        self.next_seq = 0;
        self.ait_cursor = None;
        self.translator.restore_inputs(self.sent_inputs());
        for row in backlog.rows {
            self.ait_cursor = Some((backlog.epoch.clone(), row.seq));
            for body in self.translator.item(&row.provider, &row.item) {
                self.buffer.push(body);
            }
        }
        self.translator.clear_restored();
        if self.turn_open {
            self.buffer.push(Body::Turn {
                state: TurnState::Started,
                reason: None,
            });
        }
        // Queued inputs have no timeline rows yet: keep them visible in the new generation.
        let stored = self.context.store.inputs(&self.run_id).unwrap_or_default();
        for queued in &self.queue {
            if let Some(input) = stored
                .iter()
                .find(|input| input.input_id == queued.input_id)
            {
                self.buffer.push(Body::Input {
                    id: input.input_id.clone(),
                    text: input.text.clone(),
                    by: input
                        .by
                        .clone()
                        .unwrap_or_else(|| "runtime:local".to_owned()),
                    login: input.login.clone(),
                    origin: Origin::User,
                });
            }
        }
        for ask in self.asks.values() {
            if ask.state != AskState::Resolved {
                self.buffer.push(ask.spec.body());
            }
        }
        self.write_buffer();
        self.context.outbox.send(Outgoing::Epoch {
            run_id: self.run_id.clone(),
            epoch: self.epoch.clone(),
            next: self.next_seq,
        });
    }

    fn close(&mut self, reason: &str) {
        if self.closed {
            return;
        }
        self.withdraw_open_asks();
        if self.turn_open {
            self.push(Body::Turn {
                state: TurnState::Aborted,
                reason: Some(reason.to_owned()),
            });
            self.turn_open = false;
        }
        while let Some(queued) = self.queue.pop_front() {
            let _ = self
                .context
                .store
                .set_input_state(&self.run_id, &queued.input_id, "rejected");
            self.push(Body::InputRejected {
                id: queued.input_id,
                code: RejectCode::Closed,
                reason: None,
            });
        }
        self.push(Body::Closed {
            reason: Some(reason.to_owned()),
        });
        self.closed = true;
        self.write_buffer();
        self.context.outbox.send(Outgoing::Flush {
            run_id: self.run_id.clone(),
        });
    }

    fn fail(&mut self, code: ReasonCode, detail: &str) {
        self.push(Body::Closed {
            reason: Some("error".to_owned()),
        });
        self.closed = true;
        self.flush();
        self.report(RunState::Failed, Some(code), Some(detail.to_owned()));
    }

    fn push(&mut self, body: Body) {
        if let Body::Text { mid, text } = &body {
            self.messages.append(mid, text);
        }
        self.buffer.push(body);
        if self.flush_at.is_none() {
            self.flush_at = Some(Instant::now() + COALESCE);
        }
    }

    /// Number, persist and announce everything pending.
    fn flush(&mut self) {
        self.flush_at = None;
        if self.write_buffer() {
            self.context.outbox.send(Outgoing::Live {
                run_id: self.run_id.clone(),
            });
        }
    }

    fn write_buffer(&mut self) -> bool {
        let bodies = self.buffer.take();
        if bodies.is_empty() {
            return false;
        }
        let at = now_millis();
        let mut events = Vec::with_capacity(bodies.len());
        for (offset, body) in bodies.into_iter().enumerate() {
            let event = fit(Event {
                seq: self.next_seq + offset as u64,
                at,
                body,
            });
            if let Ok(json) = event.to_json() {
                events.push(json);
            } else {
                tracing::error!("a session event could not be serialized");
            }
        }
        let cursor = self
            .ait_cursor
            .as_ref()
            .map(|(epoch, seq)| (epoch.as_str(), *seq));
        match self
            .context
            .store
            .append(&self.run_id, &self.epoch, self.next_seq, &events, cursor)
        {
            Ok(()) => {
                self.next_seq += events.len() as u64;
                true
            }
            Err(error) => {
                tracing::error!(
                    ?error,
                    run_id = self.run_id,
                    "persisting session events failed"
                );
                false
            }
        }
    }

    /// The inputs this adapter delivered, keyed by the message ID AIT echoes, as their events.
    fn sent_inputs(&self) -> Vec<(String, Body)> {
        let Ok(Some(run)) = self.context.store.run(&self.run_id) else {
            return Vec::new();
        };
        let dispatch = &run.dispatch;
        let mut inputs = vec![(
            uuid_of(&format!("{}:dispatch", self.run_id)),
            Body::Input {
                id: format!("dispatch-{}", dispatch_suffix(&self.run_id)),
                text: prompt::dispatch_input(dispatch),
                by: dispatch.requested_by.id.clone(),
                login: dispatch.requested_by.login.clone(),
                origin: Origin::Dispatch,
            },
        )];
        for input in self.context.store.inputs(&self.run_id).unwrap_or_default() {
            if input.state != "sent" {
                continue;
            }
            inputs.push((
                input.message_id,
                Body::Input {
                    id: input.input_id,
                    text: input.text,
                    by: input.by.unwrap_or_else(|| "runtime:local".to_owned()),
                    login: input.login,
                    origin: Origin::User,
                },
            ));
        }
        inputs
    }

    fn persist_ask(&self, spec: &AskSpec, state: &AskState) {
        let (label, by, effect) = match state {
            AskState::Pending { .. } => ("pending", None, None),
            AskState::Resolving { by, effect } => (
                "resolving",
                serde_json::to_string(by).ok(),
                Some(
                    if *effect == Effect::Allow {
                        "allow"
                    } else {
                        "deny"
                    }
                    .to_owned(),
                ),
            ),
            AskState::Resolved => ("resolved", None, None),
        };
        let record = AskRecord {
            ask_id: spec.id.clone(),
            spec: serde_json::to_string(spec).unwrap_or_default(),
            state: label.to_owned(),
            resolving_by: by,
            resolving_effect: effect,
        };
        if let Err(error) = self.context.store.put_ask(&self.run_id, &record) {
            tracing::error!(?error, "recording a request failed");
        }
    }

    fn cancel_requested(&self) -> bool {
        self.context
            .store
            .run(&self.run_id)
            .ok()
            .flatten()
            .is_some_and(|run| run.cancel_requested)
    }

    fn owner(&self) -> Person {
        self.translator.owner().cloned().unwrap_or_else(|| Person {
            id: "runtime:local".to_owned(),
            login: None,
        })
    }

    /// Advance the stored state and, when it moved, send the status.
    fn report(
        &self,
        status: RunState,
        reason_code: Option<ReasonCode>,
        reason_detail: Option<String>,
    ) {
        let final_text = status
            .is_terminal()
            .then(|| self.messages.final_text())
            .flatten();
        let reason_detail =
            reason_detail.map(|detail| truncate(&detail, REASON_DETAIL_BYTES).0.to_owned());
        let update = StatusUpdate {
            status,
            reason_code,
            reason_detail: reason_detail.clone(),
            final_text: final_text.clone(),
            at: now_millis(),
        };
        match self.context.store.advance(&self.run_id, &update) {
            Ok(true) => self.context.outbox.send(Outgoing::Status(Status {
                kind: "run.status",
                run_id: self.run_id.clone(),
                status,
                at: update.at,
                execution: Some(self.execution.clone()),
                reason_code,
                reason_detail,
                final_text,
            })),
            Ok(false) => {}
            Err(error) => tracing::error!(?error, "recording the run state failed"),
        }
    }
}

fn outcome_of(effect: Effect) -> Outcome {
    match effect {
        Effect::Allow => Outcome::Allow,
        Effect::Deny => Outcome::Deny,
    }
}

/// Assistant messages by `mid` (each kept to the `final_text` budget), and which came last.
///
/// Text with the same `mid` appends to its message even with other entries in between.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Messages {
    texts: HashMap<String, String>,
    last: Option<String>,
}

impl Messages {
    /// Append a text delta to its message.
    pub(crate) fn append(&mut self, mid: &str, text: &str) {
        let entry = self.texts.entry(mid.to_owned()).or_default();
        if entry.len() < crate::wire::FINAL_TEXT_BYTES {
            entry.push_str(text);
        }
        self.last = Some(mid.to_owned());
    }

    /// Start of the last message, within the `final_text` limit.
    pub(crate) fn final_text(&self) -> Option<String> {
        let text = self.texts.get(self.last.as_ref()?)?;
        translate::final_text(text)
    }
}

/// Replace logged events `[from, to]` with one error notice in a new epoch, so a frame Bonsai
/// rejected (`4400`) is never replayed again. Returns the new epoch and log end.
pub(crate) fn quarantine(
    store: &Store,
    run_id: &str,
    epoch: &str,
    from: u64,
    to: u64,
) -> Option<(String, u64)> {
    let run = store.run(run_id).ok().flatten()?;
    if run.epoch != epoch || from > to || to >= run.next_seq {
        return None;
    }
    let events = store.events(run_id, epoch, 0, run.next_seq).ok()?;
    let skipped = to - from + 1;
    let at = now_millis();
    let mut rebuilt = Vec::with_capacity(events.len());
    for (index, json) in events.into_iter().enumerate() {
        let index = index as u64;
        if index == from {
            let notice = Event {
                seq: from,
                at,
                body: Body::Notice {
                    level: Level::Error,
                    text: format!("这里有 {skipped} 个事件发不出去,已略过"),
                },
            };
            rebuilt.push(notice.to_json().ok()?);
        }
        if (from..=to).contains(&index) {
            continue;
        }
        let mut value: Value = serde_json::from_str(&json).ok()?;
        let seq = if index > to {
            index - skipped + 1
        } else {
            index
        };
        value["seq"] = Value::from(seq);
        rebuilt.push(value.to_string());
    }
    let new = new_epoch();
    store.rewrite(run_id, &new, &rebuilt).ok()?;
    tracing::error!(
        run_id,
        from,
        to,
        "Bonsai rejected session events; they were replaced in a new epoch"
    );
    Some((new, rebuilt.len() as u64))
}

/// The first eight characters of a run's hex part, for the dispatch input's ID.
fn dispatch_suffix(run_id: &str) -> String {
    run_id.trim_start_matches("r_").chars().take(8).collect()
}

/// A fresh opaque log generation: `e-` plus 12 hex digits.
pub(crate) fn new_epoch() -> String {
    let id = uuid::Uuid::new_v4().simple().to_string();
    format!("e-{}", &id[..12])
}

/// Local Unix time in milliseconds.
pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

async fn next_event(observation: Option<&mut Observation>) -> Option<HostEvent> {
    match observation {
        Some(observation) => observation.next().await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests;
