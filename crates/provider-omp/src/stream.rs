use std::borrow::Cow;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;

use base64::Engine;
use sha2::{Digest, Sha256};

use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

use provider::local::images::{ImageStore, UNAVAILABLE_IMAGE};
use provider::local::usage::requests;
use provider_core::ports::agent_session::AgentTurnEvent;
use provider_core::protocol::{timeline::NativeItem, usage::AgentUsage};

use super::events;
use super::tool_detail::{self, Report, Status};

const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;
/// Builtin command outputs held until the native run starts or the local prompt settles.
const MAX_BUFFERED_OUTPUTS: usize = 64;
/// Client identities of user messages kept for the persistence handle; older ones are dropped.
pub(super) const MAX_INPUT_IDS: usize = 4096;

/// Payload-free reasons why a native event cannot be projected.
#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("Invalid or missing OMP field: {0}.")]
    Field(&'static str),
    #[error("OMP exceeded the limit of 4096 pending tool calls.")]
    ToolLimit,
    #[error("OMP returned invalid token usage or cost.")]
    Usage,
    #[error("Ait could not render an OMP image.")]
    Image,
}

#[derive(Debug)]
pub(super) struct Stream {
    pub(super) events: VecDeque<AgentTurnEvent>,
    pub(super) input_ids: BTreeMap<String, Value>,
    /// Native request identity and optional client identity, in input admission order.
    pub(super) pending_inputs: VecDeque<(String, Option<String>)>,
    pub(super) failed: bool,
    pub(super) aborted: bool,
    pub(super) last_text: Option<String>,
    session: String,
    turn: Option<String>,
    current: Option<(String, String)>,
    counts: BTreeMap<String, usize>,
    tools: BTreeMap<String, Tool>,
    observation: u64,
    totals: AgentUsage,
    images: ImageStore,
    blobs: Option<PathBuf>,
    replaying: bool,
    output: PromptOutput,
    identities: Identities,
    todos: Todos,
    compactions: Compactions,
    /// The first child each running `task` call started, by call id (D16).
    cards: BTreeMap<String, Card>,
}

/// The child that names a running `task` card, as Paseo's subagent card tracker keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Card {
    /// The child's index among the task's children; the lowest names the card.
    index: u64,
    /// OMP's job id for the child.
    job: String,
    description: Option<String>,
}

/// Compaction rows shown so far (M3).
#[derive(Debug, Default)]
struct Compactions {
    /// Compaction id of the open (loading) row.
    open: Option<String>,
    /// An Ait-issued `/compact` reports its own rows, so OMP's compaction lifecycle events are
    /// not shown while it runs (M5).
    manual: bool,
}

/// A native tool call awaiting its `toolResult` message.
#[derive(Debug)]
pub(super) struct Tool {
    /// The assistant's call block, `{id, name, arguments}`.
    call: Value,
    /// The item OMP's `tool_execution_end` completed, shown if the turn ends before the
    /// result message.
    ended: Option<Value>,
}

impl Tool {
    fn new(call: Value) -> Self {
        Self { call, ended: None }
    }
}

/// The todo lists shown so far, so a consecutive duplicate is not shown again (M4).
#[derive(Debug, Default)]
struct Todos {
    /// The last list any source showed.
    shown: Option<Value>,
    /// The last list a todo tool result showed. History replays only tool results, so these
    /// are compared only with each other and replay shows the same rows as live output.
    recorded: Option<Value>,
}

/// Identity of the user messages shown so far (D9).
#[derive(Debug, Default)]
struct Identities {
    /// Native entry ids of the user messages already shown.
    native: BTreeSet<String>,
    /// Anchor and text digest of each user-attributed message already shown, recognising an
    /// OMP re-emission that carries no native id.
    shown: BTreeSet<(String, u64)>,
    /// Shown user messages still awaiting their native entry id, oldest first.
    unresolved: VecDeque<Unresolved>,
    /// The owner resolves native user message ids with `get_branch_messages`.
    lookups: bool,
}

/// A user message held until OMP's `get_branch_messages` names its native entry (D9).
#[derive(Debug)]
struct Unresolved {
    /// Timeline key of the held message.
    key: String,
    /// The message text as `get_branch_messages` reports it.
    text: String,
}

/// A prompt OMP handled locally and did not echo, awaiting its local echo (D8).
#[derive(Debug)]
struct LocalPrompt {
    /// The echo's client identity, kept apart from the pending inputs that later user
    /// messages, such as an accepted steer, consume.
    client_message_id: Option<String>,
}

/// The current turn's prompt echo and builtin command output (D8, D12).
#[derive(Debug, Default)]
struct PromptOutput {
    /// Text Ait submitted for the turn, echoed if OMP handles it locally.
    text: Option<String>,
    /// OMP handled the prompt locally (`agentInvoked: false`) without echoing it.
    local: Option<LocalPrompt>,
    /// The turn's native run started (`agent_start`).
    started: bool,
    /// Command output that arrived before the native run started.
    held: Vec<String>,
}

impl Stream {
    /// Token and cost sums of the assistant messages seen, with the last message's total as
    /// the context used. Only an OMP without `get_session_stats` reports usage from these.
    pub(super) fn totals(&self) -> &AgentUsage {
        &self.totals
    }
    pub(super) fn new(session: String, images: ImageStore) -> Self {
        Self {
            events: VecDeque::new(),
            input_ids: BTreeMap::new(),
            pending_inputs: VecDeque::new(),
            failed: false,
            aborted: false,
            last_text: None,
            session,
            turn: None,
            current: None,
            counts: BTreeMap::new(),
            tools: BTreeMap::new(),
            observation: 0,
            totals: AgentUsage::default(),
            images,
            blobs: None,
            replaying: false,
            output: PromptOutput::default(),
            identities: Identities::default(),
            todos: Todos::default(),
            compactions: Compactions::default(),
            cards: BTreeMap::new(),
        }
    }

    /// Hold each live user message that arrives without a native entry id until the owner
    /// resolves it with [`Stream::resolve_identities`] (D9).
    pub(super) fn with_identity_lookups(mut self) -> Self {
        self.identities.lookups = true;
        self
    }

    /// Continue in native session `session`, which OMP switched to. Messages, todos, usage
    /// totals and client identities shown so far belong to the previous session; the queued
    /// events and the current turn's state carry across. Held user messages must already be
    /// resolved and the previous session's open tools closed.
    pub(super) fn switch_session(&mut self, session: String) {
        let mut next = Self::new(session, self.images.clone());
        next.blobs = self.blobs.take();
        next.identities.lookups = self.identities.lookups;
        next.events = std::mem::take(&mut self.events);
        next.pending_inputs = std::mem::take(&mut self.pending_inputs);
        next.failed = self.failed;
        next.aborted = self.aborted;
        next.last_text = self.last_text.take();
        next.turn = self.turn.take();
        next.output = std::mem::take(&mut self.output);
        *self = next;
    }

    /// The next event to publish, keeping native order: none while the oldest held user
    /// message, at the front, awaits its native entry id.
    pub(super) fn next_event(&mut self) -> Option<AgentTurnEvent> {
        if let (Some(held), Some(AgentTurnEvent::Timeline(entry))) =
            (self.identities.unresolved.front(), self.events.front())
            && entry.key == held.key
        {
            return None;
        }
        self.events.pop_front()
    }

    /// Number of held user messages awaiting their native entry id.
    pub(super) fn unresolved(&self) -> usize {
        self.identities.unresolved.len()
    }

    /// Release the `count` oldest held user messages, naming each by the native entry that
    /// `messages`, a `get_branch_messages` reply, lists for its text, as Paseo does. Without
    /// a usable reply, or when the newest entry with that text is already shown, a message
    /// keeps its anchor key as `messageId`.
    pub(super) fn resolve_identities(&mut self, count: usize, messages: Option<&Value>) {
        let count = count.min(self.identities.unresolved.len());
        let held: Vec<_> = self.identities.unresolved.drain(..count).collect();
        let Some(messages) = messages.and_then(Value::as_array) else {
            return;
        };
        // OMP lists every user entry in file order and records an entry before announcing
        // it, so the newest entry with a held message's text is that message's own entry.
        let mut entries: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for message in messages {
            if let (Some(id), Some(text)) = (message["entryId"].as_str(), message["text"].as_str())
                && !id.is_empty()
            {
                entries.entry(text).or_default().push(id);
            }
        }
        for message in held.iter().rev() {
            let Some(candidates) = entries.get_mut(message.text.as_str()) else {
                continue;
            };
            let Some(id) = candidates.pop() else {
                continue;
            };
            if self.identities.native.contains(id) {
                // An older entry would be a guess; the anchor key stays.
                candidates.clear();
                continue;
            }
            if self.name_message(&message.key, id) {
                self.identities.native.insert(id.to_owned());
            }
        }
    }

    /// Set the queued user message `key`'s `messageId` to native entry `id`.
    fn name_message(&mut self, key: &str, id: &str) -> bool {
        self.events.iter_mut().any(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.key == key => {
                entry.item["messageId"] = json!(id);
                true
            }
            _ => false,
        })
    }

    /// Resolve native content-addressed images from `directory`, without changing OMP files.
    pub(super) fn with_blobs(mut self, directory: PathBuf) -> Self {
        self.blobs = Some(directory);
        self
    }

    /// While replaying persisted history, show an image that cannot be materialized as a
    /// placeholder; live output keeps failing on invalid images.
    pub(super) fn replay_images(&mut self, replaying: bool) {
        self.replaying = replaying;
    }

    fn display_image(&self, image: &Value) -> Result<String, Error> {
        match self.image(image) {
            Err(Error::Image) if self.replaying => Ok(UNAVAILABLE_IMAGE.to_owned()),
            result => result,
        }
    }

    fn image(&self, image: &Value) -> Result<String, Error> {
        let Some(hash) = image["data"]
            .as_str()
            .and_then(|data| data.strip_prefix("blob:sha256:"))
        else {
            return self.images.render(image).map_err(|_| Error::Image);
        };
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Image);
        }
        let path = self.blobs.as_ref().ok_or(Error::Image)?.join(hash);
        let metadata = path.symlink_metadata().map_err(|_| Error::Image)?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            return Err(Error::Image);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| Error::Image)?
            .take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Image)?;
        if bytes.len() as u64 > MAX_IMAGE_BYTES || format!("{:x}", Sha256::digest(&bytes)) != hash {
            return Err(Error::Image);
        }
        let mut resolved = Cow::Borrowed(image);
        resolved.to_mut()["data"] = json!(base64::engine::general_purpose::STANDARD.encode(bytes));
        self.images.render(&resolved).map_err(|_| Error::Image)
    }

    /// Start display state for `turn`; `prompt` is the text Ait submitted, if any.
    pub(super) fn begin(&mut self, turn: String, prompt: Option<String>) {
        self.turn = Some(turn);
        self.failed = false;
        self.aborted = false;
        self.last_text = None;
        self.output = PromptOutput {
            text: prompt,
            ..PromptOutput::default()
        };
    }

    /// Mark the turn's native run as started and show command output held until now.
    pub(super) fn start_run(&mut self) {
        self.output.started = true;
        self.flush_outputs();
    }

    /// Record that OMP handled the prompt written as `request` locally (`agentInvoked:
    /// false`). Unless a native user message already consumed its pending input, that input
    /// leaves the queue, so a later message cannot take its client identity, and the identity
    /// is kept for the prompt's local echo.
    pub(super) fn handled_locally(&mut self, request: &str) {
        if let Some(position) = self.pending_inputs.iter().position(|(id, _)| id == request)
            && let Some((_, client_message_id)) = self.pending_inputs.remove(position)
        {
            self.output.local = Some(LocalPrompt { client_message_id });
        }
    }

    /// Settle the turn's prompt output. A prompt OMP handled locally and never echoed as a
    /// native user message is echoed with its client identity (D8); held output follows.
    pub(super) fn settle_prompt(&mut self) {
        if let Some(LocalPrompt { client_message_id }) = self.output.local.take()
            && let Some(text) = self.output.text.take()
        {
            let key = format!("native:omp:{}:local:{}", self.session, uuid::Uuid::new_v4());
            let mut item = json!({"type":"user_message","messageId":key,"text":text});
            if let Some(id) = client_message_id {
                item["clientMessageId"] = json!(id);
            }
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key,
                turn_id: None,
                timestamp: Utc::now().to_rfc3339(),
                item,
            }));
        }
        self.flush_outputs();
    }

    /// Show command output held while the native run had not started.
    fn flush_outputs(&mut self) {
        for text in std::mem::take(&mut self.output.held) {
            self.command_text(&text);
        }
    }

    /// Close every tool call left without a result message when the turn ended: one OMP
    /// ended keeps its reported outcome, any other is canceled. A result can no longer
    /// follow, so the calls are forgotten.
    pub(super) fn terminalize(&mut self) {
        self.cards.clear();
        let timestamp = Utc::now().to_rfc3339();
        for (id, tool) in std::mem::take(&mut self.tools) {
            let Some(item) = tool
                .ended
                .or_else(|| tool_detail::item(&tool.call, None, Status::Canceled))
            else {
                continue;
            };
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key: self.tool_key(&id),
                turn_id: None,
                timestamp: timestamp.clone(),
                item,
            }));
        }
    }

    fn tool_key(&self, id: &str) -> String {
        format!("native:omp:{}:tool:{id}", self.session)
    }

    fn anchor(&self, message: &Value) -> Result<String, Error> {
        let role =
            super::config::text(message, "role").map_err(|_| Error::Field("message.role"))?;
        let timestamp = message["timestamp"]
            .as_i64()
            .ok_or(Error::Field("message.timestamp"))?;
        Ok(format!("native:omp:{}:{role}:{timestamp}", self.session))
    }

    fn start(&mut self, message: &Value) -> Result<String, Error> {
        let anchor = self.anchor(message)?;
        let count = self.counts.entry(anchor.clone()).or_default();
        let key = format!("{anchor}:{}", *count);
        *count += 1;
        self.current = Some((anchor, key.clone()));
        Ok(key)
    }

    fn message_key(&mut self, message: &Value) -> Result<String, Error> {
        let anchor = self.anchor(message)?;
        if let Some((current, key)) = &self.current
            && current == &anchor
        {
            return Ok(key.clone());
        }
        self.start(message)
    }

    fn item(key: String, message: &Value, item: Value) -> Result<NativeItem, Error> {
        Ok(NativeItem {
            key,
            turn_id: None,
            timestamp: message_time(message)?,
            item,
        })
    }

    pub(super) fn text(&self, content: &Value) -> Result<String, Error> {
        if let Some(text) = content.as_str() {
            return Ok(preview(text));
        }
        let mut parts = Vec::new();
        for block in content.as_array().ok_or(Error::Field("content"))? {
            match block["type"].as_str() {
                Some("text") => parts.push(preview(
                    block["text"].as_str().ok_or(Error::Field("content.text"))?,
                )),
                Some("image") => parts.push(self.display_image(block)?),
                _ => {}
            }
        }
        Ok(preview(&parts.join("\n")))
    }

    /// Project one finished native message. A user message carrying OMP's entry id (`id` or
    /// `entryId`) shows it as `messageId`; one without waits for [`Stream::resolve_identities`]
    /// when identity lookups are enabled.
    pub(super) fn message(&mut self, message: &Value) -> Result<(), Error> {
        self.project_message(message, native_id(message))
    }

    /// Project one message replayed from the session file, whose record `entry` id is the
    /// native identity of a user message.
    pub(super) fn replayed(&mut self, message: &Value, entry: &str) -> Result<(), Error> {
        self.project_message(message, Some(entry))
    }

    fn project_message(&mut self, message: &Value, native: Option<&str>) -> Result<(), Error> {
        let role = message["role"]
            .as_str()
            .ok_or(Error::Field("message.role"))?;
        // Paseo's history roles (D18), shared with live output so both show the same rows.
        let result = match role {
            "custom" if message["display"] == false => return Ok(()),
            "custom" => self.custom_message(message),
            "user" => self.user_message(message, native),
            "assistant" => self.assistant_message(message),
            "toolResult" => self.tool_result(message),
            "bashExecution" => self.bash_execution(message),
            "system" | "pythonExecution" | "hookMessage" | "branchSummary"
            | "compactionSummary" | "fileMention" => return Ok(()),
            "developer" if reminder_only(&message["content"]) => return Ok(()),
            _ => {
                let fallback = json!({"role":"custom","timestamp":message["timestamp"],
                    "display":true,"content":fallback_text(role, message)});
                self.custom_message(&fallback)
            }
        };
        self.current = None;
        result
    }

    /// Show a user's `!command` run (OMP's `bashExecution` message) as a shell row, as
    /// Paseo's `mapBashExecutionMessage` does.
    fn bash_execution(&mut self, message: &Value) -> Result<(), Error> {
        let timestamp = message["timestamp"]
            .as_i64()
            .ok_or(Error::Field("message.timestamp"))?;
        let call = format!("omp-bash-{timestamp}");
        let mut detail = json!({"type":"shell",
            "command":preview(message["command"].as_str().unwrap_or_default())});
        if let Some(output) = message["output"].as_str() {
            detail["output"] = json!(preview(output));
        }
        detail["exitCode"] = Some(&message["exitCode"])
            .filter(|code| code.is_number())
            .cloned()
            .unwrap_or(Value::Null);
        let status = if message["cancelled"] == true {
            "canceled"
        } else {
            "completed"
        };
        let item = json!({"type":"tool_call","callId":call,"name":"bash","status":status,
            "detail":detail,"error":null});
        let entry = Self::item(self.tool_key(&call), message, item)?;
        self.events.push_back(AgentTurnEvent::Timeline(entry));
        Ok(())
    }

    /// Show each of a replayed assistant message's tool calls that has no result on the
    /// branch as a running card (D18); a later live result completes the same row.
    pub(super) fn running_calls(&mut self, message: &Value, results: &HashSet<&str>) {
        let timestamp = message_time(message).unwrap_or_else(|_| Utc::now().to_rfc3339());
        for block in message["content"].as_array().into_iter().flatten() {
            let Some(id) = block["id"].as_str().filter(|_| block["type"] == "toolCall") else {
                continue;
            };
            if results.contains(id) {
                continue;
            }
            let Some(mut item) = tool_detail::item(block, None, Status::Running) else {
                continue;
            };
            self.task_card(id, &mut item, None);
            self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
                key: self.tool_key(id),
                turn_id: None,
                timestamp: timestamp.clone(),
                item,
            }));
        }
    }

    /// Show a recorded `compaction` entry as a completed compaction row (D18).
    pub(super) fn compaction(&mut self, entry: &Value) {
        let Some(id) = entry["id"].as_str() else {
            return;
        };
        let timestamp = entry["timestamp"]
            .as_str()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map_or_else(
                || Utc::now().to_rfc3339(),
                |time| time.with_timezone(&Utc).to_rfc3339(),
            );
        self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
            key: format!("native:omp:{}:compaction:{id}", self.session),
            turn_id: None,
            timestamp,
            item: events::compaction_entry(entry),
        }));
    }

    /// Show the todo list OMP's state reports (`get_state.todoPhases`), unless the last list
    /// shown already matches it, as Paseo appends it to a live session's history (M4).
    pub(super) fn state_todos(&mut self, state: &Value) {
        let Some(item) = events::todo_phases(&state["todoPhases"]) else {
            return;
        };
        if self.todos.shown.as_ref() == Some(&item) {
            return;
        }
        self.todos.shown = Some(item.clone());
        let key = self.notice_key();
        self.push_notice(key, item);
    }

    /// Record that `job`, the child at `index` of the running `task` call `call`, started,
    /// as Paseo's subagent card tracker does: the lowest index names the card's child. A
    /// change re-shows the card while the call runs.
    pub(super) fn task_child(
        &mut self,
        call: &str,
        index: u64,
        job: &str,
        description: Option<&str>,
    ) {
        let description = description
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        let next = match self.cards.get(call) {
            Some(first) if index > first.index => return,
            Some(first) if index == first.index => Card {
                index,
                job: job.to_owned(),
                description: description.or_else(|| first.description.clone()),
            },
            _ => Card {
                index,
                job: job.to_owned(),
                description,
            },
        };
        if self.cards.get(call) == Some(&next) {
            return;
        }
        let Some(tool) = self.tools.get(call).filter(|tool| tool.ended.is_none()) else {
            return;
        };
        let call_block = tool.call.clone();
        self.cards.insert(call.to_owned(), next);
        let Some(mut item) = tool_detail::item(&call_block, None, Status::Running) else {
            return;
        };
        self.task_card(call, &mut item, None);
        self.progress(NativeItem {
            key: self.tool_key(call),
            turn_id: None,
            timestamp: Utc::now().to_rfc3339(),
            item,
        });
    }

    /// Name a `task` card's first child by its qualified identity: the job OMP's result
    /// reports, else, while the call runs, the child its events reported. A running card
    /// also takes that child's description when the call has none.
    fn task_card(&self, call: &str, item: &mut Value, report: Option<Report<'_>>) {
        if item["detail"]["type"] != "sub_agent" {
            return;
        }
        let card = (item["status"] == "running")
            .then(|| self.cards.get(call))
            .flatten();
        let job = report
            .and_then(|report| tool_detail::task_child(&report.result["details"]))
            .or_else(|| card.map(|card| card.job.as_str()));
        if let Some(job) = job {
            item["detail"]["childSessionId"] = json!(format!("{}/{job}", self.session));
        }
        if item["detail"]["description"].is_null()
            && let Some(description) = card.and_then(|card| card.description.as_deref())
        {
            item["detail"]["description"] = json!(preview(description));
        }
    }

    /// Show a user message once (D9): OMP may announce the same entry again, which is
    /// recognised by its native id, or without one by its anchor and text.
    fn user_message(&mut self, message: &Value, native: Option<&str>) -> Result<(), Error> {
        let text = branch_text(&message["content"]);
        let shown = (self.anchor(message)?, digest(&text));
        let repeated = native.map_or_else(
            || self.identities.shown.contains(&shown),
            |id| self.identities.native.contains(id),
        );
        if repeated {
            return Ok(());
        }
        let key = self.message_key(message)?;
        let display = self.text(&message["content"])?;
        let item = self.user_item(&key, native.unwrap_or(&key), &display);
        let entry = Self::item(key, message, item)?;
        self.identities.shown.insert(shown);
        match native {
            Some(id) => {
                self.identities.native.insert(id.to_owned());
            }
            None if self.identities.lookups => self.identities.unresolved.push_back(Unresolved {
                key: entry.key.clone(),
                text: text.into_owned(),
            }),
            None => {}
        }
        self.events.push_back(AgentTurnEvent::Timeline(entry));
        Ok(())
    }

    /// A user message item that consumes the oldest pending input's client identity.
    fn user_item(&mut self, key: &str, message_id: &str, text: &str) -> Value {
        let mut item = json!({"type":"user_message","messageId":message_id,"text":text});
        if let Some((_, Some(id))) = self.pending_inputs.pop_front() {
            self.input_ids.insert(key.to_owned(), json!(id));
            retain_newest_inputs(&mut self.input_ids);
        }
        if let Some(id) = self.input_ids.get(key) {
            item["clientMessageId"] = id.clone();
        }
        item
    }

    fn custom_message(&mut self, message: &Value) -> Result<(), Error> {
        if let Some(text) = skill_prompt(message) {
            return self.skill_prompt(message, &text);
        }
        let text = self.text(&message["content"])?;
        if text.is_empty() {
            return Ok(());
        }
        let key = self.message_key(message)?;
        // The row's key is deterministic, so live output and replay share its call id.
        let item = events::custom(message, &text, &key);
        self.events
            .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
        Ok(())
    }

    /// Show OMP's expansion of a user's `/skill:` prompt as that user's message, as Paseo
    /// does, so it consumes the prompt's pending input. OMP gives the expansion no entry id
    /// in live events, so live output and history both use its anchor key.
    fn skill_prompt(&mut self, message: &Value, text: &str) -> Result<(), Error> {
        let shown = (self.anchor(message)?, digest(text));
        if self.identities.shown.contains(&shown) {
            return Ok(());
        }
        let key = self.message_key(message)?;
        let item = self.user_item(&key, &key, text);
        let entry = Self::item(key, message, item)?;
        self.identities.shown.insert(shown);
        self.events.push_back(AgentTurnEvent::Timeline(entry));
        Ok(())
    }

    fn assistant_message(&mut self, message: &Value) -> Result<(), Error> {
        let key = self.message_key(message)?;
        for (index, block) in message["content"]
            .as_array()
            .ok_or(Error::Field("message.content"))?
            .iter()
            .enumerate()
        {
            match block["type"].as_str() {
                Some("text" | "thinking") => {
                    let thinking = block["type"] == "thinking";
                    let text = block[if thinking { "thinking" } else { "text" }]
                        .as_str()
                        .ok_or(Error::Field("message.content.text/thinking"))?;
                    if text.is_empty() {
                        continue;
                    }
                    if !thinking {
                        self.last_text = Some(preview(text));
                    }
                    let key = format!("{key}:{index}");
                    let item = json!({"type":if thinking{"reasoning"}else{"assistant_message"},"messageId":key,"text":preview(text)});
                    self.events
                        .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
                }
                Some("toolCall") => {
                    let id = super::config::text(block, "id")
                        .map_err(|_| Error::Field("toolCall.id"))?
                        .to_owned();
                    if self.tools.len() >= 4096 && !self.tools.contains_key(&id) {
                        return Err(Error::ToolLimit);
                    }
                    match self.tools.entry(id) {
                        Entry::Occupied(mut tool) => tool.get_mut().call = block.clone(),
                        Entry::Vacant(tool) => {
                            tool.insert(Tool::new(block.clone()));
                        }
                    }
                }
                Some("image") => {
                    let key = format!("{key}:{index}");
                    let image = self.display_image(block)?;
                    let item = json!({"type":"assistant_message","messageId":key,"text":image});
                    self.events
                        .push_back(AgentTurnEvent::Timeline(Self::item(key, message, item)?));
                }
                _ => {}
            }
        }
        self.failed = message["stopReason"] == "error";
        self.aborted |= message["stopReason"] == "aborted";
        if let Some(usage) = message.get("usage") {
            self.add_totals(usage)?;
            self.push_request(&key, message);
        }
        Ok(())
    }

    /// Queue the ledger segment of a live assistant message ahead of the turn's terminal event.
    ///
    /// Replayed history and sub-agent streams have no turn and never reach the ledger.
    fn push_request(&mut self, key: &str, message: &Value) {
        let Some(turn) = &self.turn else {
            return;
        };
        let identity = requests::response_id(message).map_or_else(
            || {
                let count = key.rsplit(':').next().unwrap_or_default();
                format!("{}:{count}", message["timestamp"])
            },
            str::to_owned,
        );
        if let Some(request) = requests::pi_request("omp", turn, &identity, message) {
            self.events.push_back(AgentTurnEvent::RequestUsage(request));
        }
    }

    /// Show a tool's `toolResult` message, as live output and history replay both do: the
    /// completed or failed row, or for the todo tool its todo list (M4).
    fn tool_result(&mut self, message: &Value) -> Result<(), Error> {
        let id = super::config::text(message, "toolCallId")
            .map_err(|_| Error::Field("message.toolCallId"))?;
        // Like Paseo, a result without a known call still shows under its native name.
        let call = self.tools.remove(id).map_or_else(
            || json!({"id":id,"name":message["toolName"],"arguments":null}),
            |tool| tool.call,
        );
        let name = call["name"].as_str().unwrap_or_default();
        if name == "todo" {
            return self.recorded_todo(id, message);
        }
        let text = self.result_text(message)?;
        let report = Report {
            result: message,
            text: text.as_deref(),
        };
        let status = if tool_detail::failed(name, report, message["isError"] == true) {
            Status::Failed
        } else {
            Status::Completed
        };
        self.cards.remove(id);
        if let Some(mut item) = tool_detail::item(&call, Some(report), status) {
            self.task_card(id, &mut item, Some(report));
            let entry = Self::item(self.tool_key(id), message, item)?;
            self.events.push_back(AgentTurnEvent::Timeline(entry));
        }
        Ok(())
    }

    /// Display text of a tool result's `content`, or none when it has no text.
    fn result_text(&self, result: &Value) -> Result<Option<String>, Error> {
        match result.get("content") {
            Some(content) if !content.is_null() => {
                Ok(Some(self.text(content)?).filter(|text| !text.is_empty()))
            }
            _ => Ok(None),
        }
    }

    /// Show a native tool's lifecycle as running progress, completed at once by
    /// `tool_execution_end` with the outcome its `toolResult` message will record (D10).
    fn tool_execution(&mut self, event: &Value) -> Result<(), Error> {
        let id =
            super::config::text(event, "toolCallId").map_err(|_| Error::Field("toolCallId"))?;
        let end = event["type"] == "tool_execution_end";
        match self.tools.get(id) {
            // OMP keeps reporting a detached job's output after the call ended.
            Some(tool) if tool.ended.is_some() => return Ok(()),
            None if event["type"] == "tool_execution_update" => return Ok(()),
            Some(_) => {}
            None => {
                // Track the started call so an interrupted turn can close it.
                if self.tools.len() >= 4096 {
                    return Err(Error::ToolLimit);
                }
                let call = json!({"id":id,"name":event["toolName"],
                    "arguments":event.get("args").unwrap_or(&Value::Null)});
                self.tools.insert(id.to_owned(), Tool::new(call));
            }
        }
        let result = &event[if end { "result" } else { "partialResult" }];
        let text = self.result_text(result)?;
        let report = result.is_object().then_some(Report {
            result,
            text: text.as_deref(),
        });
        let tool = self.tools.get(id).ok_or(Error::Field("toolCallId"))?;
        let name = tool.call["name"].as_str().unwrap_or_default();
        let status = match report {
            Some(report) if end && tool_detail::failed(name, report, event["isError"] == true) => {
                Status::Failed
            }
            _ if end => Status::Completed,
            _ => Status::Running,
        };
        let Some(mut item) = tool_detail::item(&tool.call, report, status) else {
            return Ok(());
        };
        self.task_card(id, &mut item, report);
        if let Some(tool) = self.tools.get_mut(id).filter(|_| end) {
            tool.ended = Some(item.clone());
        }
        self.progress(NativeItem {
            key: self.tool_key(id),
            turn_id: None,
            timestamp: Utc::now().to_rfc3339(),
            item,
        });
        Ok(())
    }

    /// Show the todo list a todo tool result records, keyed by its call for replay.
    fn recorded_todo(&mut self, id: &str, message: &Value) -> Result<(), Error> {
        let Some(item) = events::todo_phases(&message["details"]["phases"]) else {
            return Ok(());
        };
        if self.todos.recorded.as_ref() == Some(&item) {
            return Ok(());
        }
        self.todos.recorded = Some(item.clone());
        self.todos.shown = Some(item.clone());
        let key = format!("native:omp:{}:todo:{id}", self.session);
        let entry = Self::item(key, message, item)?;
        self.events.push_back(AgentTurnEvent::Timeline(entry));
        Ok(())
    }

    /// Show the open todos a `todo_reminder` lists, unless the last list showed them.
    fn todo_reminder(&mut self, event: &Value) {
        let Some(item) = events::todo_reminder(event) else {
            return;
        };
        if self.todos.shown.as_ref() == Some(&item) {
            return;
        }
        self.todos.shown = Some(item.clone());
        let key = self.notice_key();
        self.push_notice(key, item);
    }

    /// Show the loading row of an Ait-issued `/compact` and hide OMP's own compaction
    /// events until [`Stream::end_manual_compaction`], as Paseo does (M5). Returns the
    /// compaction id both rows share.
    pub(super) fn begin_manual_compaction(&mut self) -> String {
        self.compactions.manual = true;
        let id = format!("omp-compaction:{}", uuid::Uuid::new_v4());
        let mut item = events::compaction(false, None, Some("manual"));
        item["compactionId"] = json!(id);
        let key = self.notice_key();
        self.push_notice(key, item);
        id
    }

    /// Complete the `/compact` row `id`, with OMP's `result` (`tokensBefore`) when it
    /// compacted, and show OMP's compaction events again.
    pub(super) fn end_manual_compaction(&mut self, id: &str, result: Option<&Value>) {
        self.compactions.manual = false;
        let mut item = events::compaction(true, result, Some("manual"));
        item["compactionId"] = json!(id);
        let key = self.notice_key();
        self.push_notice(key, item);
    }

    /// Show an adapter-owned answer to a slash command, such as `/autocompact`, as the
    /// assistant message Paseo emits. It never becomes the turn's final text.
    pub(super) fn command_note(&mut self, text: &str) {
        let key = self.notice_key();
        self.push_notice(
            key.clone(),
            json!({"type":"assistant_message","messageId":key,"text":preview(text),
                "metadata":{"nativeType":"slash_command","transient":true}}),
        );
    }

    /// Show an OMP telemetry event (M2, M3); a compaction's start and end share an id.
    fn telemetry(&mut self, event: &Value) {
        if self.compactions.manual && events::manual_compaction(event) {
            return;
        }
        let Some(mut item) = events::telemetry(event) else {
            return;
        };
        if item["type"] == "compaction" {
            let id = if item["status"] == "loading" {
                None
            } else {
                self.compactions.open.take()
            };
            let id = id.unwrap_or_else(|| format!("omp-compaction:{}", uuid::Uuid::new_v4()));
            if item["status"] == "loading" {
                self.compactions.open = Some(id.clone());
            }
            item["compactionId"] = json!(id);
        }
        let key = self.notice_key();
        self.push_notice(key, item);
    }

    pub(super) fn consume(&mut self, event: &Value) -> Result<(), Error> {
        match event["type"].as_str() {
            Some("extension_ui_request") if event["method"] == "notify" => self.notice(event)?,
            Some("extension_ui_request") if event["method"] == "open_url" => self.open_url(event),
            Some("message_start")
                if matches!(
                    event["message"]["role"].as_str(),
                    Some("user" | "assistant" | "toolResult" | "custom")
                ) =>
            {
                self.start(&event["message"])?;
            }
            Some("message_end") => self.message(&event["message"])?,
            Some("message_update") => {
                let update = &event["assistantMessageEvent"];
                if matches!(
                    update["type"].as_str(),
                    Some("text_delta" | "thinking_delta")
                ) {
                    let message = &event["message"];
                    let base = self.message_key(message)?;
                    let index = update["contentIndex"]
                        .as_u64()
                        .ok_or(Error::Field("assistantMessageEvent.contentIndex"))?;
                    let key = format!("{base}:{index}");
                    let item = json!({"type":if update["type"]=="thinking_delta"{"reasoning"}else{"assistant_message"},
                        "messageId":key,"text":preview(update["delta"].as_str().ok_or(Error::Field("assistantMessageEvent.delta"))?)});
                    let entry = Self::item(key, message, item)?;
                    self.progress(entry);
                }
            }
            Some("tool_execution_start" | "tool_execution_update" | "tool_execution_end") => {
                self.tool_execution(event)?;
            }
            Some("todo_reminder") => self.todo_reminder(event),
            // AgentSession owns the settled/abort lifecycle; an agent_end may precede retry.
            _ => self.telemetry(event),
        }
        Ok(())
    }

    /// Show an extension `notify` request as a notification row.
    fn notice(&mut self, event: &Value) -> Result<(), Error> {
        let text = event["message"].as_str().ok_or(Error::Field("message"))?;
        if text.is_empty() {
            return Ok(());
        }
        let text = preview(text);
        if self.last_text.is_none() {
            self.last_text = Some(text.clone());
        }
        let level = match event["notifyType"].as_str() {
            Some("warning") => "warning",
            Some("error") => "error",
            _ => "info",
        };
        let metadata = json!({"nativeType":event["type"],
            "notificationType":event["notifyType"],"transient":true});
        self.notification(level, text, metadata);
        Ok(())
    }

    /// Show OMP's `open_url` request (M1) as an assistant message with the link, the launch
    /// URL and instructions, as Paseo does. OMP expects no reply. Only an http(s) URL becomes
    /// a link.
    fn open_url(&mut self, event: &Value) {
        let Some(url) = event["url"].as_str() else {
            return;
        };
        let mut lines = Vec::with_capacity(5);
        let link = format!("[Open URL]({url})");
        if url.starts_with("https://") || url.starts_with("http://") {
            lines.push(link.as_str());
        }
        let location = format!("URL: {url}");
        lines.push(&location);
        let launch = event["launchUrl"]
            .as_str()
            .map(|launch| format!("Launch URL: {launch}"));
        lines.extend(launch.as_deref());
        if let Some(instructions) = event["instructions"].as_str() {
            lines.extend(["", instructions]);
        }
        let key = self.notice_key();
        self.push_notice(
            key.clone(),
            json!({"type":"assistant_message","messageId":key,"text":preview(&lines.join("\n")),
                "metadata":{"nativeType":"open_url","transient":true}}),
        );
    }

    /// Show an adapter-owned notification at `level` in the current turn.
    pub(super) fn notify(&mut self, level: &str, text: String) {
        self.notification(level, text, json!({"transient":true}));
    }

    fn notification(&mut self, level: &str, text: String, metadata: Value) {
        let key = self.notice_key();
        let mut item = json!({"type":"notification","level":level});
        item["message"] = Value::String(text);
        item["metadata"] = metadata;
        self.push_notice(key, item);
    }

    /// Accept OMP's builtin command output (D12) with ANSI escapes removed and whitespace
    /// trimmed. Output that arrives before the native run starts is held, as Paseo does.
    pub(super) fn command_output(&mut self, event: &Value) -> Result<(), Error> {
        let text = event["text"].as_str().ok_or(Error::Field("text"))?;
        let text = strip_ansi(text);
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let text = preview(text);
        if self.output.started {
            self.command_text(&text);
        } else if self.output.held.len() < MAX_BUFFERED_OUTPUTS {
            self.output.held.push(text);
        } else {
            tracing::debug!("Dropping OMP command output beyond the buffered limit");
        }
        Ok(())
    }

    fn command_text(&mut self, text: &str) {
        self.last_text = Some(text.to_owned());
        let key = self.notice_key();
        self.push_notice(
            key.clone(),
            json!({"type":"assistant_message","messageId":key,"text":text,
                "metadata":{"nativeType":"command_output","transient":true}}),
        );
    }

    fn notice_key(&self) -> String {
        format!(
            "native:omp:{}:notice:{}",
            self.session,
            uuid::Uuid::new_v4()
        )
    }

    fn push_notice(&mut self, key: String, item: Value) {
        self.events.push_back(AgentTurnEvent::Timeline(NativeItem {
            key,
            turn_id: self.turn.clone(),
            timestamp: Utc::now().to_rfc3339(),
            item,
        }));
    }

    fn progress(&mut self, entry: NativeItem) {
        self.observation += 1;
        self.events.push_back(AgentTurnEvent::Progress {
            observation: format!(
                "{}:{}:{}",
                self.session,
                self.turn.as_deref().unwrap_or("history"),
                self.observation
            ),
            entry,
        });
    }

    /// Add one message's `usage` to the totals. The session's usage reads publish usage
    /// (D26); invalid native usage still fails the projection.
    fn add_totals(&mut self, usage: &Value) -> Result<(), Error> {
        let input = usage["input"].as_u64().ok_or(Error::Field("usage.input"))?;
        let output = usage["output"]
            .as_u64()
            .ok_or(Error::Field("usage.output"))?;
        let cached = usage["cacheRead"].as_u64().unwrap_or_default();
        let totals = &mut self.totals;
        totals.input_tokens = Some(
            totals
                .input_tokens
                .unwrap_or_default()
                .saturating_add(input),
        );
        totals.output_tokens = Some(
            totals
                .output_tokens
                .unwrap_or_default()
                .saturating_add(output),
        );
        totals.cached_input_tokens = Some(
            totals
                .cached_input_tokens
                .unwrap_or_default()
                .saturating_add(cached),
        );
        totals.context_window_used_tokens = usage["totalTokens"].as_u64();
        if let Some(cost) = usage["cost"]["total"].as_f64() {
            totals.total_cost_usd = Some(totals.total_cost_usd.unwrap_or_default() + cost);
        }
        if !totals.is_valid() {
            return Err(Error::Usage);
        }
        Ok(())
    }
}

/// Keep the [`MAX_INPUT_IDS`] newest client identities in `ids`, keyed by message keys
/// (`native:omp:<session>:<role>:<timestamp>:<n>`), ordered by timestamp, then index.
/// A key without that suffix counts as the oldest.
pub(super) fn retain_newest_inputs(ids: &mut BTreeMap<String, Value>) {
    let excess = ids.len().saturating_sub(MAX_INPUT_IDS);
    if excess == 0 {
        return;
    }
    let mut keys: Vec<_> = ids.keys().map(|key| (input_order(key), key)).collect();
    keys.select_nth_unstable(excess - 1);
    let oldest: Vec<String> = keys[..excess]
        .iter()
        .map(|(_, key)| (*key).clone())
        .collect();
    for key in oldest {
        ids.remove(&key);
    }
}

/// The message timestamp and index at the end of a message key.
fn input_order(key: &str) -> (i64, u64) {
    let mut parts = key.rsplitn(3, ':');
    let index = parts.next().and_then(|index| index.parse().ok());
    match (parts.next().and_then(|time| time.parse().ok()), index) {
        (Some(time), Some(index)) => (time, index),
        _ => (i64::MIN, 0),
    }
}

/// A message's epoch-millisecond `timestamp` in RFC 3339.
fn message_time(message: &Value) -> Result<String, Error> {
    Ok(message["timestamp"]
        .as_i64()
        .and_then(|time| Utc.timestamp_millis_opt(time).single())
        .ok_or(Error::Field("message.timestamp"))?
        .to_rfc3339())
}

/// Whether `content` is only an OMP `<system-reminder>`, which Paseo's history drops; OMP's
/// rule-violation reminder carries attributes (`<system-reminder reason="…">`).
fn reminder_only(content: &Value) -> bool {
    let [block] = content.as_array().map(Vec::as_slice).unwrap_or_default() else {
        return false;
    };
    let Some(text) = block["text"].as_str().filter(|_| block["type"] == "text") else {
        return false;
    };
    let text = text.trim();
    text.strip_prefix("<system-reminder")
        .is_some_and(|rest| rest.starts_with(|next: char| next == '>' || next.is_whitespace()))
        && text.ends_with("</system-reminder>")
}

/// Paseo's `visibleFallback`: `[<kind>] <text>` for a record Ait has no row for, with its
/// `content` text, else its `text`, else a placeholder.
pub(super) fn fallback_text(kind: &str, value: &Value) -> String {
    const UNSUPPORTED: &str = "Unsupported history record";
    let text = match &value["content"] {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            let joined = blocks
                .iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            if joined.is_empty() {
                UNSUPPORTED.to_owned()
            } else {
                joined
            }
        }
        _ => value["text"].as_str().unwrap_or(UNSUPPORTED).to_owned(),
    };
    preview(&format!("[{kind}] {text}"))
}

/// OMP's native entry id on a live message (`id`, else `entryId`), as Paseo reads it.
fn native_id(message: &Value) -> Option<&str> {
    [&message["id"], &message["entryId"]]
        .into_iter()
        .find_map(Value::as_str)
        .filter(|id| !id.is_empty())
}

/// A message's text as OMP's `get_branch_messages` reports it: its text blocks joined
/// without a separator.
fn branch_text(content: &Value) -> Cow<'_, str> {
    match content {
        Value::String(text) => Cow::Borrowed(text),
        Value::Array(blocks) => Cow::Owned(
            blocks
                .iter()
                .filter(|block| block["type"] == "text")
                .filter_map(|block| block["text"].as_str())
                .collect(),
        ),
        _ => Cow::Borrowed(""),
    }
}

/// An in-process digest of `text`, so shown messages are remembered without their text.
fn digest(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// The user's typed `/skill:<name> <args>` for OMP's user-attributed `skill-prompt`
/// expansion, rebuilt from its details as Paseo's `ompSkillPromptUserText` does.
fn skill_prompt(message: &Value) -> Option<String> {
    if message["customType"] != "skill-prompt" || message["attribution"] != "user" {
        return None;
    }
    let details = &message["details"];
    let name = details["name"].as_str().filter(|name| !name.is_empty())?;
    let text = match details["args"].as_str().map(str::trim) {
        Some(args) if !args.is_empty() => format!("/skill:{name} {args}"),
        _ => format!("/skill:{name}"),
    };
    Some(preview(&text))
}

/// Remove ANSI terminal escape sequences (CSI, OSC and two-character escapes) from `text`.
fn strip_ansi(text: &str) -> Cow<'_, str> {
    if !text.contains('\u{1b}') {
        return Cow::Borrowed(text);
    }
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }
        match characters.next() {
            // Control sequence: parameter and intermediate bytes, then one final byte.
            Some('[') => {
                for character in characters.by_ref() {
                    if ('@'..='~').contains(&character) {
                        break;
                    }
                }
            }
            // Operating system command: terminated by BEL or ESC \.
            Some(']') => {
                while let Some(character) = characters.next() {
                    if character == '\u{7}' {
                        break;
                    }
                    if character == '\u{1b}' {
                        characters.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            // Character set designation: one more byte names the set.
            Some('(' | ')' | '*' | '+') => {
                characters.next();
            }
            _ => {}
        }
    }
    Cow::Owned(output)
}

pub(super) fn preview(text: &str) -> String {
    let end = text.floor_char_boundary(text.len().min(128 * 1024));
    if end == text.len() {
        text.to_owned()
    } else {
        format!("{}\n[Truncated; full output remains in OMP.]", &text[..end])
    }
}

#[cfg(test)]
mod tests;
