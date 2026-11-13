//! Pi BTW side-question runner: one stateless auxiliary RPC process per turn
//! (integrated with the shared BTW lifecycle).
//!
//! The runner is never the main native writer. Each auxiliary process is
//! launched with Pi's public denial flags only:
//!
//! ```text
//! pi --mode rpc --no-session --no-tools --no-extensions --no-mcp --no-skills
//!    --no-context-files
//! ```
//!
//! `--no-session` keeps Ait's daemon topic store the only persistence: no
//! native session file, no auth or config copy, nothing Pi-owned survives the
//! turn. Every process receives the frozen [`SideContextSnapshot`] plus the
//! stored question/answer turns rendered verbatim into the prompt text, so a
//! follow-up is wire-identical whether or not a previous process is still
//! alive.
//!
//! Isolation is enforced on the wire, not assumed from flags: Pi's public RPC
//! `bash` command executes even under `--no-tools`, so every native write goes
//! through [`request`], which rejects any type outside [`ALLOWED`]. There is no
//! generic passthrough, no extension, and no execution-capable command.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::{
    PiClient, config,
    transport::{self, Transport},
};
use provider_core::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider_core::ports::diagnostics::{Category, Diagnostic};
use provider_core::ports::side_questions::{
    SideAsk, SideAskReceipt, SideCancelOutcome, SideCapability, SideQuestions, SideRunnerSpec,
};
use provider_core::protocol::side_questions::{
    self, BtwHistoryRecord, BtwHistoryTurn, MAX_ANSWER_BYTES, MAX_QUESTION_BYTES,
    SideContextSnapshot, SideQuestionsState, SideStrategy, SideTopic, SideTurnStatus,
};

/// Strategy frozen for every Pi topic: provider-local isolated execution.
const STRATEGY: SideStrategy = SideStrategy::Isolated;

/// Public denial flags: no native session, tools, extensions, MCP servers,
/// skills or project context files (AGENTS.md/CLAUDE.md).
const ISOLATION: [&str; 6] = [
    "--no-session",
    "--no-tools",
    "--no-extensions",
    "--no-mcp",
    "--no-skills",
    "--no-context-files",
];

/// Closed native feature set. Any other request type is a programming error.
const ALLOWED: [&str; 5] = [
    "get_state",
    "set_model",
    "set_thinking_level",
    "prompt",
    "abort",
];

/// Frame polling interval while an auxiliary turn runs.
const FRAME_INTERVAL: Duration = Duration::from_millis(10);
/// Native abort settlement budget, matching the main writer.
const ABORT_SETTLEMENT_TIMEOUT: Duration = Duration::from_secs(10);
/// Shutdown budget for one auxiliary process group.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Settle the native send before the runtime's independent ten-second admission deadline.
const ADMISSION_TIMEOUT: Duration = Duration::from_secs(8);
/// Hard byte budget for one rendered prompt (frozen snapshot plus prior turns).
const MAX_PROMPT_BYTES: usize = 96 * 1024;
const CONTEXT_LIMIT_MESSAGE: &str =
    "BTW context limit reached; expand this side conversation into a full session to continue.";

#[derive(Debug, thiserror::Error)]
enum ComposeError {
    #[error("invalid side question or topic")]
    Invalid,
    #[error("side conversation exceeds the prompt budget")]
    ContextLimit,
}

/// Whether the closed channel admits native request `kind`.
fn allowed(kind: &str) -> bool {
    ALLOWED.contains(&kind)
}

/// Send one allowlisted control request over an auxiliary transport.
async fn request(
    transport: &mut Transport,
    kind: &str,
    mut payload: Value,
) -> Result<Value, AgentSessionError> {
    if !allowed(kind) {
        return Err(AgentSessionError::Rejected);
    }
    payload["type"] = json!(kind);
    transport.request(payload).await
}

/// Command accepted by an auxiliary process task.
enum Control {
    /// Run one question turn; admission is acknowledged on the native response.
    Prompt {
        text: String,
        /// Model frozen in the topic snapshot; never a host default.
        model: String,
        ack: oneshot::Sender<Result<(), AgentSessionError>>,
    },
    /// Abort the running turn and confirm native settlement.
    Abort {
        ack: oneshot::Sender<Result<(), AgentSessionError>>,
    },
}

/// Published state of one topic, shared between its task and the runner.
#[derive(Debug)]
struct Live {
    topic: SideTopic,
    generation: u64,
}

/// One topic's auxiliary process, present only while its turn runs.
#[derive(Debug)]
struct Auxiliary {
    live: Arc<Mutex<Live>>,
    control: mpsc::UnboundedSender<Control>,
    task: JoinHandle<Result<(), AgentSessionError>>,
}

impl Auxiliary {
    fn topic(&self) -> SideTopic {
        lock(&self.live).topic.clone()
    }

    fn running(&self) -> bool {
        lock(&self.live).topic.latest_turn().status == SideTurnStatus::Running
    }
}

/// Pi's implementation of the shared BTW auxiliary port.
#[derive(Debug)]
pub(super) struct Runner {
    client: PiClient,
    session: AgentSessionSpec,
    directory: PathBuf,
    thinking: Option<String>,
    /// Retained topic records, newest first is applied at projection time.
    topics: BTreeMap<String, Arc<Mutex<Live>>>,
    /// Auxiliary process per still-running topic; idle topics keep no process.
    auxiliary: BTreeMap<String, Auxiliary>,
    /// Request identity to topic identity, so an uncertain ask can be reconciled.
    requests: BTreeMap<String, String>,
    observed: u64,
    reveal: Option<String>,
    capability: SideCapability,
    context_limited: bool,
}

impl Runner {
    /// Build a runner for one Agent. Performs no inference and writes nothing.
    ///
    ///
    /// # Errors
    /// Returns `Rejected` for an invalid specification or a missing selected
    /// model.
    pub(super) fn new(
        mut client: PiClient,
        spec: &SideRunnerSpec,
    ) -> Result<Self, AgentSessionError> {
        config::spec(&spec.session)?;
        if let Some(model) = &spec.session.config.model
            && config::split_model(model).is_none()
        {
            return Err(AgentSessionError::Rejected);
        }
        client.deadline = client.deadline.min(ADMISSION_TIMEOUT);
        client.admission_deadline = client.admission_deadline.min(ADMISSION_TIMEOUT);
        Ok(Self {
            client,
            session: spec.session.clone(),
            directory: spec.directory.clone(),
            thinking: spec.session.config.thinking_option_id.clone(),
            topics: BTreeMap::new(),
            auxiliary: BTreeMap::new(),
            requests: BTreeMap::new(),
            observed: 0,
            reveal: None,
            capability: SideCapability::unavailable("Pi side questions were not probed"),
            context_limited: false,
        })
    }

    /// Verify the installed runtime without inference.
    ///
    /// # Errors
    /// Returns `Unavailable` when Pi is missing or older than 1.2.
    pub(super) async fn probe(&mut self) -> Result<(), AgentSessionError> {
        self.capability = if !cfg!(unix) {
            SideCapability::unavailable("Pi side-question recovery requires Unix process groups")
        } else if provider_native::configuration::executable(&self.client.program) {
            match transport::version(&self.client).await {
                Ok(()) => SideCapability::available(),
                Err(AgentSessionError::Unavailable) => {
                    SideCapability::unavailable("Pi 1.2 or newer is required for side questions")
                }
                Err(_) => SideCapability::unavailable("Pi version probe failed"),
            }
        } else {
            SideCapability::unavailable("Pi executable is unavailable")
        };
        if self.capability.available {
            Ok(())
        } else {
            Err(AgentSessionError::Unavailable)
        }
    }

    fn running_id(&self) -> Option<String> {
        self.auxiliary
            .iter()
            .find(|(_, auxiliary)| auxiliary.running())
            .map(|(id, _)| id.clone())
    }

    /// Admit one ask and return the running topic once Pi acknowledged it.
    async fn start(&mut self, ask: SideAsk) -> Result<SideAskReceipt, AgentSessionError> {
        if !self.capability.available {
            return Err(AgentSessionError::Unavailable);
        }
        if self.running_id().is_some() {
            return Err(AgentSessionError::Busy);
        }
        let question = ask.question.trim();
        if question.is_empty() || question.len() > MAX_QUESTION_BYTES {
            return Err(AgentSessionError::Rejected);
        }
        let (topic, text) = Self::compose(&ask, question).map_err(|error| {
            self.context_limited = matches!(error, ComposeError::ContextLimit);
            AgentSessionError::Rejected
        })?;
        // A stateless runner follows the snapshot's selected model; a missing
        // selection is rejected upstream and never replaced by a host default.
        let model = ask
            .snapshot
            .model
            .clone()
            .ok_or(AgentSessionError::Rejected)?;
        let id = topic.record.id.clone();
        let live = Arc::new(Mutex::new(Live {
            topic,
            generation: 1,
        }));
        let auxiliary = self.launch(live, &ask.request_id)?;
        let (ack, admitted) = oneshot::channel();
        if auxiliary
            .control
            .send(Control::Prompt { text, model, ack })
            .is_err()
        {
            release(auxiliary).await;
            return Err(AgentSessionError::Failed);
        }
        // Register ownership before the first admission await. A runtime deadline
        // can then cancel this future without detaching an unreachable native task.
        self.requests.insert(ask.request_id.clone(), id.clone());
        let previous_live = self.topics.insert(id.clone(), auxiliary.live.clone());
        self.auxiliary.insert(id.clone(), auxiliary);
        let admission = admitted.await.unwrap_or(Err(AgentSessionError::Failed));
        if matches!(admission, Err(AgentSessionError::Rejected)) {
            self.requests.remove(&ask.request_id);
            if let Some(previous) = previous_live {
                self.topics.insert(id.clone(), previous);
            } else {
                self.topics.remove(&id);
            }
            if let Some(auxiliary) = self.auxiliary.remove(&id) {
                release(auxiliary).await;
            }
            return Err(AgentSessionError::Rejected);
        }
        let topic = self
            .auxiliary
            .get(&id)
            .ok_or(AgentSessionError::Failed)?
            .topic();
        self.reveal = Some(id.clone());
        // A lost native acknowledgement may follow an accepted prompt. Keep
        // its identity and partial result for reconciliation, without replay.
        admission?;
        Ok(SideAskReceipt {
            strategy: STRATEGY,
            topic,
        })
    }

    /// Build the topic record and rendered prompt for one ask.
    fn compose(ask: &SideAsk, question: &str) -> Result<(SideTopic, String), ComposeError> {
        let now = now_millis();
        if let Some(topic_id) = ask.record_id.as_deref() {
            let previous = ask.previous.as_ref().ok_or(ComposeError::Invalid)?;
            if previous.record.id != topic_id
                || previous.latest_turn().status != SideTurnStatus::Complete
            {
                return Err(ComposeError::Invalid);
            }
            let text = render(&ask.snapshot, Some(previous), question)?;
            return Ok((follow_up(previous.clone(), question, now), text));
        }
        let id = Uuid::new_v4().to_string();
        let topic = SideTopic {
            record: BtwHistoryRecord {
                id,
                question: question.to_owned(),
                images: Vec::new(),
                answer: String::new(),
                status: SideTurnStatus::Running,
                created_at: now,
                updated_at: now,
                error: None,
                leaf_id: None,
                title: None,
                follow_ups: Vec::new(),
            },
            strategy: STRATEGY,
            truncated: ask.snapshot.truncated,
        };
        let text = render(&ask.snapshot, None, question)?;
        Ok((topic, text))
    }

    /// Launch an auxiliary task against a freshly published topic record.
    fn launch(
        &self,
        live: Arc<Mutex<Live>>,
        request_id: &str,
    ) -> Result<Auxiliary, AgentSessionError> {
        let args: Vec<OsString> = ISOLATION.iter().map(OsString::from).collect();
        let transport = Transport::spawn(
            &self.client,
            &self.session.cwd,
            &args,
            self.session.confinement.as_ref(),
        )
        .map_err(|_| AgentSessionError::Unavailable)?;
        let marker = provider_native::auxiliary::path(&self.directory, request_id);
        provider_native::auxiliary::create(
            &marker,
            request_id,
            &lock(&live).topic.record.id,
            "pi-stateless",
            transport.group_id().ok_or(AgentSessionError::Unavailable)?,
        )?;
        let published = live.clone();
        let (control, commands) = mpsc::unbounded_channel();
        let thinking = self.thinking.clone();
        let request_id = request_id.to_owned();
        let task = tokio::spawn(async move {
            run(transport, thinking, published, commands).await?;
            provider_native::auxiliary::reconcile(&marker, &request_id)
        });
        Ok(Auxiliary {
            live,
            control,
            task,
        })
    }

    fn projection(&self) -> SideQuestionsState {
        let mut records: Vec<SideTopic> = self
            .topics
            .values()
            .map(|live| lock(live).topic.clone())
            .collect();
        sort_newest_first(&mut records);
        let omitted = records.len().saturating_sub(side_questions::MAX_TOPICS);
        records.truncate(side_questions::MAX_TOPICS);
        SideQuestionsState {
            records,
            error: self
                .context_limited
                .then(|| CONTEXT_LIMIT_MESSAGE.to_owned()),
            omitted,
            reveal: self.reveal.clone(),
            running_id: self.running_id(),
            ..SideQuestionsState::default()
        }
    }

    /// Forget settled auxiliary handles so later asks never reuse a process.
    ///
    /// A settled task is closing its transport; dropping the handle leaves that
    /// cleanup running, and `Drop` covers a task cancelled mid-range.
    fn reap(&mut self) {
        self.auxiliary
            .retain(|_, auxiliary| auxiliary.running() && !auxiliary.task.is_finished());
    }
}

#[async_trait::async_trait]
impl SideQuestions for Runner {
    fn side_strategy(&self) -> SideStrategy {
        STRATEGY
    }

    fn side_capability(&self) -> SideCapability {
        self.capability.clone()
    }

    async fn side_ask(&mut self, ask: SideAsk) -> Result<SideAskReceipt, AgentSessionError> {
        self.reap();
        self.context_limited = false;
        if let Some(topic_id) = self.requests.get(&ask.request_id)
            && let Some(topic) = self.topics.get(topic_id)
        {
            return Ok(SideAskReceipt {
                strategy: STRATEGY,
                topic: lock(topic).topic.clone(),
            });
        }
        self.start(ask).await
    }

    fn side_rejection_reason(&self) -> Option<&str> {
        self.context_limited.then_some("context_limit")
    }

    async fn side_cancel(
        &mut self,
        record_id: Option<&str>,
    ) -> Result<SideCancelOutcome, AgentSessionError> {
        self.reap();
        let target = match record_id {
            Some(id) => self.auxiliary.get(id).map(|_| id.to_owned()),
            None => self.running_id(),
        };
        let Some(id) = target else {
            return Ok(SideCancelOutcome { cancelled: false });
        };
        let Some(auxiliary) = self.auxiliary.get(&id) else {
            return Ok(SideCancelOutcome { cancelled: false });
        };
        let (ack, confirmed) = oneshot::channel();
        if auxiliary.control.send(Control::Abort { ack }).is_err() {
            // The process exited while the turn ran; cancellation is unconfirmed.
            return Err(AgentSessionError::Failed);
        }
        tokio::time::timeout(ABORT_SETTLEMENT_TIMEOUT, confirmed)
            .await
            .map_err(|_| AgentSessionError::Failed)?
            .map_err(|_| AgentSessionError::Failed)??;
        self.reap();
        Ok(SideCancelOutcome { cancelled: true })
    }

    /// Reconcile an uncertain ask from retained in-process topic state.
    ///
    /// `--no-session` means nothing native survives the process, so this never
    /// guesses from question text or timing: an ask this runner admitted
    /// returns its authoritative topic, and an ask it never admitted (for
    /// example after a daemon restart) returns `None`.
    async fn reconcile_side(
        &mut self,
        request_id: &str,
    ) -> Result<Option<SideTopic>, AgentSessionError> {
        self.reap();
        let Some(topic_id) = self.requests.get(request_id) else {
            let marker = provider_native::auxiliary::path(&self.directory, request_id);
            provider_native::auxiliary::reconcile(&marker, request_id)?;
            return Ok(None);
        };
        Ok(self
            .topics
            .get(topic_id)
            .map(|live| lock(live).topic.clone()))
    }

    async fn side_history(&mut self) -> Result<Vec<SideTopic>, AgentSessionError> {
        let mut topics: Vec<SideTopic> = self
            .topics
            .values()
            .map(|live| lock(live).topic.clone())
            .collect();
        sort_newest_first(&mut topics);
        Ok(topics)
    }

    fn side_projection(&self) -> SideQuestionsState {
        self.projection()
    }

    fn poll_side(&mut self) -> Option<SideQuestionsState> {
        let observed: u64 = self.topics.values().map(|live| lock(live).generation).sum();
        if observed == self.observed {
            return None;
        }
        self.observed = observed;
        self.reap();
        Some(self.projection())
    }

    async fn close_side(&mut self) -> Result<(), AgentSessionError> {
        let mut failure = None;
        for (_, auxiliary) in std::mem::take(&mut self.auxiliary) {
            if let Some(error) = release(auxiliary).await {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

/// Shut an auxiliary task down, dropping its owned process group.
async fn release(auxiliary: Auxiliary) -> Option<AgentSessionError> {
    let Auxiliary {
        control, mut task, ..
    } = auxiliary;
    drop(control);
    match tokio::time::timeout(CLOSE_TIMEOUT, &mut task).await {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(error))) => Some(error),
        Ok(Err(_)) => Some(AgentSessionError::Failed),
        Err(_) => {
            task.abort();
            let _ = task.await;
            Some(AgentSessionError::Failed)
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        for auxiliary in self.auxiliary.values() {
            auxiliary.task.abort();
        }
    }
}

/// Drive one auxiliary process for a single turn, then release it.
async fn run(
    mut transport: Transport,
    thinking: Option<String>,
    live: Arc<Mutex<Live>>,
    mut control: mpsc::UnboundedReceiver<Control>,
) -> Result<(), AgentSessionError> {
    let mut turn: Option<TurnState> = None;
    let mut aborting: Option<oneshot::Sender<Result<(), AgentSessionError>>> = None;
    let mut deadline = None;
    loop {
        if turn.is_none() {
            match control.recv().await {
                Some(Control::Prompt { text, model, ack }) => {
                    if configure(&mut transport, &model, thinking.as_deref())
                        .await
                        .is_err()
                    {
                        let _ = ack.send(Err(AgentSessionError::Rejected));
                        break;
                    }
                    let mut state = TurnState::default();
                    let result = transport
                        .prompt(json!({"type":"prompt","message":text}), |event| {
                            state
                                .consume(event)
                                .map(|_| ())
                                .map_err(|()| Diagnostic::new(Category::Protocol, "prompt_event"))
                        })
                        .await;
                    match result {
                        Ok(response) if response["disposition"] == "started" => {
                            publish(&live, &mut state);
                            let _ = ack.send(Ok(()));
                            turn = Some(state);
                        }
                        // A handled prompt runs no side answer and ends the process.
                        Ok(_) => {
                            let _ = ack.send(Err(AgentSessionError::Rejected));
                            break;
                        }
                        Err(error) => {
                            if matches!(error, AgentSessionError::Failed) {
                                interrupt(&live, &mut state);
                            }
                            let _ = ack.send(Err(error));
                            break;
                        }
                    }
                }
                // Cancellation and close before any turn are trivially satisfied.
                Some(Control::Abort { ack }) => {
                    let _ = ack.send(Ok(()));
                }
                None => break,
            }
            continue;
        }
        let Some(state) = turn.as_mut() else {
            break;
        };
        if drain(&mut transport, state).is_err() {
            state.interrupt();
        }
        publish(&live, state);
        if state.status().is_terminal() {
            if let Some(ack) = aborting.take() {
                let _ = ack.send(Ok(()));
            }
            break;
        }
        if let Some(limit) = deadline
            && Instant::now() >= limit
        {
            // An unconfirmed cancellation is never reported as success.
            if let Some(ack) = aborting.take() {
                let _ = ack.send(Err(AgentSessionError::Failed));
            }
            state.interrupt();
            publish(&live, state);
            break;
        }
        match tokio::time::timeout(FRAME_INTERVAL, control.recv()).await {
            Ok(None) => break,
            Ok(Some(Control::Abort { ack })) => {
                if request(&mut transport, "abort", json!({})).await.is_err() {
                    let _ = ack.send(Err(AgentSessionError::Failed));
                    break;
                }
                aborting = Some(ack);
                deadline = Some(Instant::now() + ABORT_SETTLEMENT_TIMEOUT);
            }
            Ok(Some(Control::Prompt { ack, .. })) => {
                let _ = ack.send(Err(AgentSessionError::Busy));
            }
            Err(_) => {}
        }
    }
    if let Some(state) = turn.as_mut() {
        interrupt(&live, state);
    }
    transport.close().await
}

/// Configure the selected model (and mirrored thinking level) before the prompt.
async fn configure(
    transport: &mut Transport,
    model: &str,
    thinking: Option<&str>,
) -> Result<(), AgentSessionError> {
    let (provider, id) = config::split_model(model).ok_or(AgentSessionError::Rejected)?;
    request(
        transport,
        "set_model",
        json!({"provider":provider,"modelId":id}),
    )
    .await?;
    if let Some(level) = thinking {
        request(transport, "set_thinking_level", json!({"level":level})).await?;
    }
    Ok(())
}

/// Drain already-observed native frames into the running turn.
///
/// Returns `Err(())` when the process died or a frame broke the contract; the
/// turn is then published as `interrupted` rather than resubmitted.
fn drain(transport: &mut Transport, state: &mut TurnState) -> Result<(), ()> {
    loop {
        match transport.poll() {
            Ok(Some(frame)) => {
                if state.consume(&frame)? {
                    return Ok(());
                }
            }
            Ok(None) => return Ok(()),
            Err(_) => return Err(()),
        }
    }
}

/// Bounded decoder for one auxiliary turn: answer text plus settlement only.
///
/// Display projection belongs to the daemon topic store, so the runner keeps no
/// timeline events and a long answer cannot exhaust a display queue.
#[derive(Debug)]
struct TurnState {
    text: String,
    truncated: bool,
    status: SideTurnStatus,
    error: Option<String>,
    /// Whether the published topic is behind this turn.
    dirty: bool,
}

impl Default for TurnState {
    fn default() -> Self {
        Self {
            text: String::new(),
            truncated: false,
            status: SideTurnStatus::Running,
            error: None,
            dirty: false,
        }
    }
}

impl TurnState {
    fn status(&self) -> SideTurnStatus {
        self.status
    }

    fn interrupt(&mut self) {
        if self.status == SideTurnStatus::Running {
            self.status = SideTurnStatus::Interrupted;
            self.dirty = true;
        }
    }

    fn push(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let remaining = MAX_ANSWER_BYTES.saturating_sub(self.text.len());
        if delta.len() > remaining {
            self.truncated = true;
            self.text
                .push_str(&delta[..delta.floor_char_boundary(remaining)]);
        } else {
            self.text.push_str(delta);
        }
        self.dirty = true;
    }

    /// Consume one native frame; returns `Ok(true)` once the turn settled.
    fn consume(&mut self, event: &Value) -> Result<bool, ()> {
        match event["type"].as_str() {
            Some("message_update") if event["assistantMessageEvent"]["type"] == "text_delta" => {
                let delta = event["assistantMessageEvent"]["delta"].as_str().ok_or(())?;
                self.push(delta);
            }
            Some("message_end") if event["message"]["role"] == "assistant" => {
                let blocks = event["message"]["content"].as_array().ok_or(())?;
                self.text.clear();
                for block in blocks {
                    if block["type"] == "text" {
                        let text = block["text"].as_str().ok_or(())?;
                        self.push(text);
                    }
                }
                if event["message"]["stopReason"] == "error" {
                    self.error = Some("Pi reported an error for this side question".to_owned());
                }
                self.dirty = true;
            }
            Some("agent_settled") => {
                self.status = if event["aborted"].as_bool().ok_or(())? {
                    SideTurnStatus::Cancelled
                } else if self.error.is_some() {
                    SideTurnStatus::Error
                } else {
                    SideTurnStatus::Complete
                };
                self.dirty = true;
                return Ok(true);
            }
            Some("response") | None => return Err(()),
            _ => {}
        }
        Ok(false)
    }
}

/// Publish the turn's text and lifecycle when it changed.
fn publish(live: &Arc<Mutex<Live>>, state: &mut TurnState) {
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let mut shared = lock(live);
    let topic = &mut shared.topic;
    let (answer, clipped) = side_questions::bounded(&state.text, MAX_ANSWER_BYTES);
    let now = now_millis();
    if let Some(last) = topic.record.follow_ups.last_mut() {
        answer.clone_into(&mut last.answer);
        last.status = state.status();
        last.updated_at = now;
        last.error.clone_from(&state.error);
    } else {
        answer.clone_into(&mut topic.record.answer);
        topic.record.status = state.status();
        topic.record.updated_at = now;
        topic.record.error.clone_from(&state.error);
    }
    topic.truncated |= clipped || state.truncated;
    shared.generation += 1;
}

/// Mark the turn interrupted after process death.
fn interrupt(live: &Arc<Mutex<Live>>, state: &mut TurnState) {
    state.interrupt();
    publish(live, state);
}

/// Render the frozen display snapshot verbatim, then accumulated turns, then the question.
///
/// Oversized prompts are rejected before inference rather than losing prior turns.
fn render(
    snapshot: &SideContextSnapshot,
    previous: Option<&SideTopic>,
    question: &str,
) -> Result<String, ComposeError> {
    let mut context = String::from("[BTW context snapshot v1 - frozen]\n");
    for entry in &snapshot.entries {
        context.push_str(&entry.role);
        context.push_str(": ");
        context.push_str(&entry.text);
        context.push('\n');
    }
    if snapshot.truncated {
        context.push_str("[BTW context snapshot truncated]\n");
    }
    if let Some(previous) = previous {
        context.push_str("\n[BTW prior question]\n");
        context.push_str(&previous.record.question);
        context.push_str("\n[BTW prior answer]\n");
        context.push_str(&previous.record.answer);
        context.push('\n');
        for turn in &previous.record.follow_ups {
            context.push_str("\n[BTW prior question]\n");
            context.push_str(&turn.question);
            context.push_str("\n[BTW prior answer]\n");
            context.push_str(&turn.answer);
            context.push('\n');
        }
    }
    context.push_str("\n[BTW question]\n");
    context.push_str(question);
    if context.len() > MAX_PROMPT_BYTES {
        return Err(ComposeError::ContextLimit);
    }
    Ok(context)
}

/// Append one running question turn; projection limits never erase durable turns.
fn follow_up(mut previous: SideTopic, question: &str, now: u64) -> SideTopic {
    previous.record.follow_ups.push(BtwHistoryTurn {
        question: question.to_owned(),
        images: Vec::new(),
        answer: String::new(),
        status: SideTurnStatus::Running,
        created_at: now,
        updated_at: now,
        error: None,
    });
    previous
}

/// Newest-first ordering shared by the projection and history reads.
fn sort_newest_first(topics: &mut [SideTopic]) {
    topics.sort_by(|left, right| {
        right
            .record
            .created_at
            .cmp(&left.record.created_at)
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Milliseconds since the Unix epoch; saturates instead of panicking.
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(all(test, unix))]
mod tests;
