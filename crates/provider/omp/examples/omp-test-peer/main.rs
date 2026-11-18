//! Offline OMP RPC peer for adapter and browser tests. Its state stays in the test agent directory.

#![expect(
    unused_crate_dependencies,
    reason = "The peer is an independent example and shares the adapter's manifest."
)]

mod ask;
mod commands;
mod metadata;
mod prompt;
mod scene;

use std::fs::{self, File};
use std::io::{self, BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep};

use ask::AskFlow;

const FRAME_BYTES: usize = 1_048_576;
const CHUNK_BYTES: usize = 262_144;
const RECORD_TIME: &str = "2026-10-21T00:00:01Z";

#[expect(
    clippy::struct_excessive_bools,
    reason = "Flags represent independent RPC state and fixture settings."
)]
struct Peer {
    root: PathBuf,
    cwd: PathBuf,
    file: PathBuf,
    session_id: String,
    model: Value,
    available: Vec<Value>,
    level: String,
    records: Vec<Value>,
    leaf: Option<String>,
    clock: i64,
    persist_session: bool,
    active: bool,
    fast_enabled: bool,
    pending_async: bool,
    current_prompt: String,
    prompt_id: Value,
    protocol: u8,
    auto_compaction: bool,
    todo_phases: Value,
    stream_messages: u64,
    ui_sequence: u64,
    pending_dialog: Option<Value>,
    dialog_commands: Vec<Value>,
    pending_plugin: Option<(Value, Value)>,
    pending_control: Option<Value>,
    pending_approval: bool,
    ask_flow: Option<AskFlow>,
    queued_prompts: Vec<Value>,
    queued_steers: Vec<Value>,
    pending_compaction: Option<(Instant, Value)>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        metadata::version();
        return;
    }

    let root =
        PathBuf::from(std::env::var_os("PI_CODING_AGENT_DIR").expect("test agent directory"));
    fs::create_dir_all(&root).expect("create test agent directory");
    metadata::log(&root, &json!({ "args": args }));
    if metadata::respond(&args, &root) {
        return;
    }
    metadata::probe_write_confinement(&root);
    let mut peer = Peer::open(root, &args);
    let shutdown = std::env::var("OMP_FIXTURE_SHUTDOWN");
    let hold_at_eof = matches!(shutdown.as_deref(), Ok("term" | "stubborn"));
    if hold_at_eof {
        watch_sigterm(&peer.root, shutdown.as_deref() == Ok("term"));
    }
    peer.announce_ready();
    if env_set("OMP_FIXTURE_REPLAY") && flag(&args, "--session").is_some() {
        peer.replay();
    }
    peer.serve().await;
    if hold_at_eof {
        // Exit only when the adapter signals this process.
        std::future::pending::<()>().await;
    }
}

impl Peer {
    fn open(root: PathBuf, args: &[String]) -> Self {
        let cwd = std::env::current_dir().expect("test workspace directory");
        let approval = flag(args, "--approval-mode");
        if let Some(mode) = approval
            && !["always-ask", "always-ask-cached", "write", "yolo"].contains(&mode)
        {
            refuse(
                &format!("Error: Invalid --approval-mode value: {}", json!(mode)),
                2,
            );
        }
        let level = flag(args, "--thinking").unwrap_or("off");
        if ![
            "off", "minimal", "low", "medium", "high", "xhigh", "max", "auto",
        ]
        .contains(&level)
        {
            refuse(
                &format!("Error: Invalid --thinking value: {}", json!(level)),
                2,
            );
        }
        let available = models();
        let requested = flag(args, "--model");
        let select = |route| available.iter().find(|item| model_route(item) == route);
        let (file, records, session_id) = flag(args, "--session").map_or_else(
            || new_session(&root, &cwd, "2026-10-21"),
            |file| resume_session(PathBuf::from(file)),
        );
        let recorded_model = records
            .iter()
            .rev()
            .find(|entry| entry["type"] == "model_change")
            .and_then(|entry| entry["model"].as_str());
        let model = match (requested, recorded_model) {
            (Some(route), _) => {
                select(route).unwrap_or_else(|| refuse(&format!("Model \"{route}\" not found"), 1))
            }
            (None, Some(route)) => select(route).unwrap_or_else(|| {
                refuse(
                    &format!("SessionModelRestoreError: Could not restore model {route}"),
                    1,
                )
            }),
            (None, None) => &available[0],
        }
        .clone();
        let fast_enabled = records
            .iter()
            .rev()
            .find(|entry| entry["type"] == "service_tier_change")
            .is_some_and(|entry| !entry["serviceTier"].is_null());
        let clock = 1_792_540_800_000 + i64::try_from(records.len()).expect("entry count") * 10;
        let leaf = if records.len() > 1 {
            records
                .last()
                .and_then(|entry| entry["id"].as_str())
                .map(str::to_owned)
        } else {
            None
        };
        let record_model = records.len() == 1 || requested.is_some();
        let mut peer = Self {
            root,
            cwd,
            file,
            session_id,
            model,
            available,
            level: level.to_owned(),
            records,
            leaf,
            clock,
            persist_session: !args.iter().any(|arg| arg == "--no-session"),
            active: false,
            fast_enabled,
            pending_async: false,
            current_prompt: String::new(),
            prompt_id: Value::Null,
            protocol: 1,
            auto_compaction: true,
            todo_phases: env_json("OMP_FIXTURE_TODO_PHASES", json!([])),
            stream_messages: 0,
            ui_sequence: 0,
            pending_dialog: None,
            dialog_commands: Vec::new(),
            pending_plugin: None,
            pending_control: None,
            pending_approval: false,
            ask_flow: None,
            queued_prompts: Vec::new(),
            queued_steers: Vec::new(),
            pending_compaction: None,
        };
        if record_model {
            peer.entry("model_change", json!({ "model": model_route(&peer.model) }));
        }
        peer
    }

    fn announce_ready(&self) {
        let mode = std::env::var("OMP_FIXTURE_READY");
        if mode.as_deref() == Ok("never") {
            return;
        }
        if mode.as_deref() == Ok("late") {
            self.write_line("{not json");
            self.emit(&json!({ "type": "notice", "level": "info", "message": "before ready" }));
        }
        self.emit(&json!({
            "type": "ready", "protocolVersion": 1,
            "supportedProtocolVersions": if mode.as_deref() == Ok("v1") { &[1][..] } else { &[1, 2] },
            "maxFrameBytes": if mode.as_deref() == Ok("sizes") { 2 * FRAME_BYTES } else { FRAME_BYTES },
            "maxReassembledFrameBytes": 67_108_864, "supportsBashExcludeFromContext": true,
        }));
        self.emit(&json!({ "type": "available_commands_update", "commands": [
            { "name": "compact", "description": "Compact the session context", "source": "builtin" },
            { "name": "handoff", "description": "Hand off to a fresh context", "source": "builtin" },
            { "name": "review", "description": "Review", "source": "skill" },
        ] }));
    }

    fn replay(&self) {
        for record in self
            .records
            .iter()
            .filter(|entry| entry["type"] == "message")
        {
            self.emit(&json!({ "type": "message_start", "message": record["message"] }));
            self.emit(&json!({ "type": "message_end", "message": record["message"] }));
        }
    }

    async fn serve(&mut self) {
        let mut lines = stdin_lines();
        let slow = std::env::var("OMP_FIXTURE_SLOW").unwrap_or_default();
        let unanswered = std::env::var("OMP_FIXTURE_UNANSWERED").unwrap_or_default();
        loop {
            let line = tokio::select! {
                line = lines.recv() => line,
                () = sleep(Duration::from_millis(10)), if self.pending_compaction.is_some() => {
                    self.complete_compaction();
                    continue;
                }
            };
            let Some(line) = line else { return };
            let command: Value = serde_json::from_str(&line).expect("parse OMP command");
            metadata::log(&self.root, &command);
            let kind = command["type"].as_str().expect("OMP command type");
            if slow.split(',').any(|item| item == kind) {
                sleep(Duration::from_millis(300)).await;
            }
            if !unanswered.split(',').any(|item| item == kind) && !self.dispatch(&command).await {
                return;
            }
        }
    }

    fn write_line(&self, text: &str) {
        let ending = if self.current_prompt == "garbage-line" {
            "\r\n"
        } else {
            "\n"
        };
        write_stdout(&format!("{text}{ending}"));
    }

    fn emit(&self, value: &Value) {
        let data = value.to_string();
        if self.protocol != 2 || data.len() < FRAME_BYTES {
            self.write_line(&data);
            return;
        }
        let chunk_id = format!("rpc-{}", id());
        let chunks = data.as_bytes().chunks(CHUNK_BYTES);
        let count = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            let frame = json!({
                "type": "rpc_chunk", "chunkId": chunk_id, "index": index, "count": count,
                "byteLength": data.len(), "data": STANDARD.encode(chunk),
            });
            self.write_line(&frame.to_string());
        }
    }

    fn response(&self, command: &Value, data: Value) {
        let mut frame = json!({
            "type": "response", "id": command["id"], "command": command["type"],
            "success": true, "error": null,
        });
        frame["data"] = data;
        self.emit(&frame);
    }

    fn reject(&self, command: &Value, error: &str) {
        let mut frame = json!({
            "type": "response", "id": command["id"], "command": command["type"],
            "success": false, "error": error,
        });
        if error == "Rejected" {
            frame["data"] = Value::Null;
        }
        self.emit(&frame);
    }

    fn persist(&self) {
        if self.persist_session {
            write_records(&self.file, &self.records);
        }
    }

    fn entry(&mut self, kind: &str, fields: Value) {
        self.record(kind, id(), RECORD_TIME, fields);
        if self.file.exists() {
            self.persist();
        }
    }

    fn record(&mut self, kind: &str, entry_id: String, timestamp: &str, fields: Value) {
        let mut entry = json!({
            "type": kind, "id": entry_id, "parentId": self.leaf, "timestamp": timestamp,
        });
        extend(&mut entry, fields);
        self.records.push(entry);
        self.leaf = Some(entry_id);
    }

    fn fast_active(&self) -> bool {
        self.fast_enabled && self.model["provider"] == "test"
    }

    fn announce(&mut self, message: &Value, entry_id: &str, kind: &str) {
        self.stream_messages += 1;
        let mut frame = json!({
            "type": format!("message_{kind}"),
            "messageId": format!("msg-{}", self.stream_messages),
            "message": message,
        });
        if kind == "end" && message["role"] == "user" && env_set("OMP_FIXTURE_MESSAGE_IDS") {
            frame["message"]["id"] = json!(entry_id);
        }
        self.emit(&frame);
    }

    fn message(&mut self, role: &str, content: Value, fields: Value) -> (Value, String) {
        self.clock += 10;
        let mut message = json!({ "role": role, "timestamp": self.clock });
        message["content"] = content;
        extend(&mut message, fields);
        let entry_id = id();
        self.announce(&message, &entry_id, "start");
        if role == "assistant"
            && let Some(blocks) = message["content"].as_array()
        {
            for (index, block) in blocks.iter().enumerate() {
                if let Some(field @ ("text" | "thinking")) = block["type"].as_str() {
                    self.emit(&json!({
                        "type": "message_update", "message": message,
                        "assistantMessageEvent": { "type": format!("{field}_delta"),
                            "contentIndex": index, "delta": block[field] },
                    }));
                }
            }
        }
        self.record(
            "message",
            entry_id.clone(),
            RECORD_TIME,
            json!({ "message": message }),
        );
        self.persist();
        self.announce(&message, &entry_id, "end");
        (message, entry_id)
    }

    fn custom_message(
        &mut self,
        kind: &str,
        content: Value,
        details: Value,
        attribution: &str,
        display: bool,
    ) {
        self.clock += 10;
        let mut fields = json!({
            "customType": kind, "display": display, "attribution": attribution,
        });
        fields["content"] = content;
        fields["details"] = details;
        let mut message = json!({ "role": "custom", "timestamp": self.clock });
        extend(&mut message, fields.clone());
        let entry_id = id()[..8].to_owned();
        self.announce(&message, &entry_id, "start");
        let timestamp = DateTime::<Utc>::from_timestamp_millis(self.clock)
            .expect("test message time")
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        self.record("custom_message", entry_id.clone(), &timestamp, fields);
        self.persist();
        self.announce(&message, &entry_id, "end");
    }

    fn assistant(&mut self, text: &str, stop_reason: &str) {
        self.message(
            "assistant",
            json!([{ "type": "text", "text": text }]),
            json!({ "provider": "test", "model": "model", "stopReason": stop_reason }),
        );
    }

    fn prompt_result(&self, status: &str, agent_invoked: bool, settled: bool) {
        self.result_for(&self.prompt_id, status, agent_invoked, settled);
    }

    fn result_for(&self, prompt: &Value, status: &str, agent_invoked: bool, settled: bool) {
        self.emit(
            &json!({ "type": "prompt_result", "id": prompt, "agentInvoked": agent_invoked,
            "status": status, "sessionSettled": settled }),
        );
    }

    fn finish(&mut self, settled: bool) {
        self.message(
            "assistant",
            json!([{ "type": "text", "text": "Hello world" }]),
            json!({
                "provider": "test", "model": "model", "stopReason": "stop",
                "usage": { "input": 10, "output": 5, "cacheRead": 2, "cacheWrite": 0,
                    "totalTokens": 17, "cost": { "total": 0.001 } },
            }),
        );
        self.emit(&json!({ "type": "agent_end", "messages": [] }));
        self.active = false;
        // OMP reports queued prompts before the run's own prompt.
        for queued in std::mem::take(&mut self.queued_prompts) {
            self.result_for(&queued, "completed", true, true);
        }
        self.prompt_result("completed", true, settled);
        if !settled {
            self.emit(&json!({ "type": "agent_start" }));
            self.assistant("Background finished", "stop");
            self.emit(&json!({ "type": "agent_end", "messages": [] }));
        }
        self.emit(&json!({ "type": "session_settled" }));
    }

    fn ui_request(&mut self, method: &str, fields: Value) {
        self.ui_sequence += 1;
        let mut frame = json!({
            "type": "extension_ui_request", "id": format!("ui-{}", self.ui_sequence), "method": method,
        });
        extend(&mut frame, fields);
        self.emit(&frame);
    }

    fn read_plugins(&self) -> Vec<Value> {
        let file = self.root.join("plugins.json");
        if file.exists() {
            serde_json::from_value(read_json(&file)).expect("test plugin commands")
        } else {
            Vec::new()
        }
    }

    fn current_messages(&self) -> impl DoubleEndedIterator<Item = &Value> {
        let start = self
            .records
            .iter()
            .rposition(|record| record["type"] == "compaction")
            .map_or(0, |index| index + 1);
        self.records[start..]
            .iter()
            .filter_map(|record| record.get("message"))
    }

    fn context_usage(&self) -> Value {
        let estimate = json!(8586);
        let tokens = self
            .current_messages()
            .rev()
            .find_map(|message| message.get("usage"))
            .map_or(&estimate, |usage| &usage["totalTokens"]);
        let window = &self.model["contextWindow"];
        let percent = tokens.as_f64().unwrap_or_default() * 100.0
            / window.as_f64().expect("test model context window");
        json!({ "tokens": tokens.as_i64().unwrap_or_default(), "contextWindow": window,
            "percent": percent })
    }

    fn session_stats(&self) -> Value {
        let mut user = 0;
        let mut assistant = 0;
        let mut total = 0;
        let mut cost = 0.0;
        let mut sums = [0_i64; 5];
        let fields = ["input", "output", "cacheRead", "cacheWrite", "totalTokens"];
        for message in self.current_messages() {
            user += u64::from(message["role"] == "user");
            assistant += u64::from(message["role"] == "assistant");
            total += 1;
            let usage = &message["usage"];
            for (sum, field) in sums.iter_mut().zip(fields) {
                *sum += usage[field].as_i64().unwrap_or_default();
            }
            cost += usage["cost"]["total"].as_f64().unwrap_or_default();
        }
        json!({
            "sessionFile": self.file, "sessionId": self.session_id,
            "userMessages": user, "assistantMessages": assistant, "toolCalls": 0, "toolResults": 0,
            "totalMessages": total, "tokens": { "input": sums[0], "output": sums[1],
                "cacheRead": sums[2], "cacheWrite": sums[3], "reasoning": 0, "total": sums[4] },
            "cost": cost, "premiumRequests": 0, "contextUsage": self.context_usage(),
        })
    }

    fn compact(&mut self, command: &Value) {
        self.emit(&json!({ "type": "manual_compaction_start" }));
        if self.current_messages().next().is_none() {
            let error = "Nothing to compact (session too small)";
            self.emit(&json!({ "type": "manual_compaction_end", "aborted": false,
                "skipped": true, "errorMessage": error }));
            self.reject(command, error);
            return;
        }
        self.pending_compaction =
            Some((Instant::now() + Duration::from_millis(100), command.clone()));
    }

    fn complete_compaction(&mut self) {
        let Some((ready, _)) = &self.pending_compaction else {
            return;
        };
        if Instant::now() < *ready
            || (env_set("OMP_FIXTURE_HOLD_COMPACT") && !self.root.join("release-compact").exists())
        {
            return;
        }
        let (_, command) = self.pending_compaction.take().expect("pending compaction");
        let tokens = self.context_usage()["tokens"].clone();
        let result = json!({ "summary": "Summary", "shortSummary": "Summary",
            "firstKeptEntryId": self.leaf, "tokensBefore": tokens,
            "details": { "readFiles": [], "modifiedFiles": [] } });
        self.entry(
            "compaction",
            json!({ "summary": "Summary", "firstKeptEntryId": self.leaf,
            "tokensBefore": tokens, "tokensAfter": 100, "method": "summary" }),
        );
        self.persist();
        self.emit(&json!({ "type": "manual_compaction_end", "result": result, "aborted": false }));
        self.response(&command, result);
    }
}

fn resume_session(file: PathBuf) -> (PathBuf, Vec<Value>, String) {
    if !file.exists() {
        refuse(
            &format!("Error: Session \"{}\" not found.", file.display()),
            1,
        );
    }
    let input = fs::read_to_string(&file).expect("read test session");
    let records: Vec<Value> = input
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse test session entry"))
        .filter(|entry: &Value| entry["type"] != "title")
        .collect();
    let session_id = records
        .iter()
        .find(|entry| entry["type"] == "session")
        .and_then(|entry| entry["id"].as_str())
        .expect("test session header")
        .to_owned();
    (file, records, session_id)
}

fn new_session(root: &Path, cwd: &Path, date: &str) -> (PathBuf, Vec<Value>, String) {
    let session_id = id();
    let file = root
        .join("sessions")
        .join("workspace")
        .join(format!("{date}T00-00-00-000Z_{session_id}.jsonl"));
    let records = vec![json!({ "type": "session", "version": 3, "id": session_id,
        "cwd": cwd, "timestamp": format!("{date}T00:00:00Z") })];
    (file, records, session_id)
}

fn models() -> Vec<Value> {
    let removed = std::env::var("OMP_FIXTURE_REMOVED_MODEL");
    [
        json!({ "provider": "test", "id": "model", "name": "Fixture model", "reasoning": true,
            "input": ["text", "image"], "thinking": { "mode": "effort", "efforts": ["low", "high"] },
            "contextWindow": 32768 }),
        json!({ "provider": "other", "id": "model", "name": "Other", "reasoning": true,
            "input": ["text"], "thinking": { "mode": "effort", "efforts": ["low", "high", "max"],
                "defaultLevel": "high" }, "contextWindow": 32768 }),
        json!({ "provider": "test", "id": "any-effort", "name": "vendor/Any_effort",
            "reasoning": true, "input": ["text", "image"], "contextWindow": 32768 }),
        json!({ "provider": "test", "id": "plain", "name": "Vendor: Plain", "reasoning": false,
            "input": ["text"], "contextWindow": 32768 }),
    ].into_iter().filter(|model| removed.as_deref() != Ok(&model_route(model))).collect()
}

fn model_route(model: &Value) -> String {
    let provider = model["provider"].as_str().expect("test model provider");
    let id = model["id"].as_str().expect("test model identifier");
    format!("{provider}/{id}")
}

fn branch_text(content: &Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect()
}

fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

fn env_json(name: &str, default: Value) -> Value {
    std::env::var(name).map_or(default, |value| serde_json::from_str(&value).expect(name))
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

fn refuse(text: &str, status: i32) -> ! {
    eprintln!("{text}");
    std::process::exit(status)
}

fn read_json(path: &Path) -> Value {
    let file = File::open(path).expect("open test JSON");
    serde_json::from_reader(file).expect("parse test JSON")
}

fn extend(value: &mut Value, fields: Value) {
    if let Value::Object(fields) = fields {
        value
            .as_object_mut()
            .expect("test JSON object")
            .extend(fields);
    }
}

fn write_records(path: &Path, records: &[Value]) {
    fs::create_dir_all(path.parent().expect("test session parent"))
        .expect("create sessions directory");
    let mut file = BufWriter::new(File::create(path).expect("open test session for writing"));
    for record in records {
        serde_json::to_writer(&mut file, record).expect("write test session entry");
        file.write_all(b"\n")
            .expect("write test session line ending");
    }
    file.flush().expect("flush test session");
}

fn write_stdout(text: &str) {
    let mut stdout = io::stdout().lock();
    stdout.write_all(text.as_bytes()).expect("write OMP frame");
    stdout.flush().expect("flush OMP frame");
}

fn stdin_lines() -> mpsc::Receiver<String> {
    // A dedicated blocking reader lets the peer answer while stdin is idle and terminate
    // without waiting for a blocked read when the adapter kills the process.
    let (sender, receiver) = mpsc::channel(1);
    std::thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let line = line.expect("read OMP command");
            if sender.blocking_send(line).is_err() {
                return;
            }
        }
    });
    receiver
}

#[cfg(unix)]
fn watch_sigterm(root: &Path, exit: bool) {
    use tokio::signal::unix::{SignalKind, signal};

    let mut signals = signal(SignalKind::terminate()).expect("listen for SIGTERM");
    let marker = root.join("sigterm");
    tokio::spawn(async move {
        while signals.recv().await.is_some() {
            if exit {
                File::create(&marker).expect("record SIGTERM");
                std::process::exit(0);
            }
        }
    });
}

#[cfg(not(unix))]
fn watch_sigterm(_root: &Path, _exit: bool) {}

fn kill_self() -> ! {
    #[cfg(unix)]
    {
        let status = std::process::Command::new("/bin/kill")
            .args(["-KILL", &std::process::id().to_string()])
            .status()
            .expect("send SIGKILL to test peer");
        panic!("test peer survived SIGKILL ({status})");
    }
    #[cfg(not(unix))]
    std::process::exit(137);
}
