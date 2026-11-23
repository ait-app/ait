//! Wire budgets for the Provider reads a client issues to open an Agent conversation.
//!
//! The suite builds deterministic synthetic history, runs each read through the handler that
//! answers it, and checks the encoded JSON result bytes and returned rows against `CEILINGS`.
//! Raising a ceiling is a one-line change to that table; the reason belongs in the PR.

use domain::agent_runtime::PersistedAgentRuntimeRecord;
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::workspace::registry::WorkspaceMutationContext;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider_core::protocol::timeline::NativeItem;
use serde_json::{Value, json};

use crate::protocol::agent_lifecycle::AgentListRequest;
use crate::protocol::timeline::FetchRequest;
use crate::rpc::agent_runtime::{execute, snapshot, sync_snapshot, synchronize};
use crate::rpc::timeline::fetch_stored;
use crate::service::agent_runtime::AgentRuntimeDirectory;
use crate::storage::timeline::Timeline;

/// The baseline predates the two child-control capability flags added by #673. Each Agent
/// snapshot adds exactly these bytes; keeping the adjustment explicit preserves zero headroom.
const CHILD_CONTROL_BYTES: usize =
    b",\"supportsChildGuide\":false,\"supportsChildStop\":false".len();

/// Read name, maximum encoded result bytes, maximum returned rows.
///
/// Values are the measurement on 2026-11-23 plus about 25 percent, rounded up; row ceilings
/// that equal a requested page size or a fixture size are exact. Two kinds of read keep the
/// measured value with no headroom: the synchronized cold list ignores `page.limit` and grows
/// with the host's Agents, and long-turn pages that reach a large tool output ship its whole
/// body, bounded only by the 900 KiB page budget.
const CEILINGS: &[(&str, usize, usize)] = &[
    ("agent directory: workspace page", 67_000, 40),
    (
        "agent directory: synchronized cold",
        270_188 + 200 * CHILD_CONTROL_BYTES,
        200,
    ),
    ("agent directory: synchronized resume", 2_000, 1),
    ("agent directory: get", 1_700, 1),
    ("long turn: timeline tail", 195_000, 40),
    ("long turn: older page", 417_240 + CHILD_CONTROL_BYTES, 40),
    (
        "long turn: catch-up after reconnect",
        417_239 + CHILD_CONTROL_BYTES,
        40,
    ),
    ("long turn: catch-up after epoch change", 195_000, 40),
    ("forty turns: timeline tail", 133_000, 40),
    ("forty turns: older page", 138_000, 40),
    ("forty turns: catch-up after reconnect", 131_000, 40),
    ("forty turns: catch-up after epoch change", 133_000, 40),
];

/// The page size clients request for timeline and Workspace Agent reads.
const PAGE: u64 = 40;
/// Source items in the long turn, including its user message.
const LONG_TURN_ITEMS: usize = 900;
/// Finished turns in the long-history scenario.
const FINISHED_TURNS: usize = 40;
/// Agents on the host and in the opened Workspace.
const HOST_AGENTS: usize = 200;
const WORKSPACE_AGENTS: usize = 40;
/// Source items a client misses while disconnected before it catches up.
const MISSED_ITEMS: u64 = 60;
const SMALL_OUTPUT: usize = 4 * 1024;
const LARGE_OUTPUT: usize = 256 * 1024;
const FILLER: &str = "test result: ok. 12 passed; 0 failed; src/storage/timeline.rs:42\n";

struct Measured {
    read: String,
    bytes: usize,
    rows: usize,
}

impl Measured {
    /// Measure `value`; `rows` names its row array, or a single row object.
    fn new(read: impl Into<String>, value: &Value, rows: &str) -> Self {
        Self {
            read: read.into(),
            bytes: serde_json::to_vec(value).expect("encode the result").len(),
            rows: match &value[rows] {
                Value::Array(entries) => entries.len(),
                Value::Object(_) => 1,
                _ => 0,
            },
        }
    }
}

#[test]
fn workspace_open_reads_stay_within_their_wire_budgets() {
    let mut measured = agent_directory_reads();
    measured.extend(timeline_reads("long turn", &long_turn()));
    measured.extend(timeline_reads("forty turns", &finished_turns()));
    enforce(&measured);
}

fn enforce(measured: &[Measured]) {
    let mut failures = Vec::new();
    for reading in measured {
        let Some(&(_, max_bytes, max_rows)) =
            CEILINGS.iter().find(|(read, ..)| *read == reading.read)
        else {
            failures.push(format!("{}: no ceiling in CEILINGS", reading.read));
            continue;
        };
        // The table is the suite's report; CI keeps it in the test log.
        println!(
            "wire budget | {:<42} | bytes {:>7} / {:>7} | rows {:>3} / {:>3}",
            reading.read, reading.bytes, max_bytes, reading.rows, max_rows
        );
        if reading.bytes > max_bytes || reading.rows > max_rows {
            failures.push(format!(
                "{}: {} bytes, {} rows exceed {max_bytes} bytes, {max_rows} rows",
                reading.read, reading.bytes, reading.rows
            ));
        }
    }
    for (read, ..) in CEILINGS {
        if !measured.iter().any(|reading| reading.read == *read) {
            failures.push(format!("{read}: ceiling has no measurement"));
        }
    }
    assert!(
        failures.is_empty(),
        "wire budgets exceeded:\n{}",
        failures.join("\n")
    );
}

fn agent_directory_reads() -> Vec<Measured> {
    let root = tempfile::tempdir().expect("registry root");
    let agents = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    agents.initialize().expect("initialize agents");
    let projects = FileBackedProjectRegistry::new(root.path().join("projects.json"));
    projects.initialize().expect("initialize projects");
    let workspaces = FileBackedWorkspaceRegistry::new(root.path().join("workspaces.json"));
    workspaces.initialize().expect("initialize workspaces");
    projects
        .upsert(
            &serde_json::from_value(json!({"projectId":"prj-budget","rootPath":"/repo",
            "kind":"git","displayName":"repo","createdAt":"2026-11-01T00:00:00Z",
            "updatedAt":"2026-11-01T00:00:00Z","archivedAt":null}))
            .expect("decode the project"),
        )
        .expect("register the project");
    for index in 0..HOST_AGENTS / WORKSPACE_AGENTS {
        workspaces
            .upsert(
                &serde_json::from_value(json!({"workspaceId":format!("wks-{index}"),
                    "projectId":"prj-budget","cwd":format!("/repo/.worktrees/{index}"),
                    "kind":"worktree","displayName":format!("feature-{index}"),
                    "createdAt":"2026-11-01T00:00:00Z","updatedAt":"2026-11-01T00:00:00Z",
                    "archivedAt":null}))
                .expect("decode the workspace"),
                WorkspaceMutationContext::default(),
            )
            .expect("register the workspace");
    }
    for index in 0..HOST_AGENTS {
        agents.upsert(&agent(index)).expect("register the agent");
    }
    let mut directory = AgentRuntimeDirectory::new(
        Box::new(agents.clone()),
        Box::new(workspaces),
        Box::new(projects),
    );
    let sort = json!([{"key":"updated_at","direction":"desc"}]);

    let page = execute(
        &mut directory,
        "agent.list.request",
        json!({"scope":"active","filter":{"workspaceIds":["wks-0"]},"sort":sort,
            "page":{"limit":PAGE}}),
    )
    .expect("workspace agent page");
    let cold = synchronized(
        &directory,
        json!({"scope":"active","sort":sort,"page":{"limit":HOST_AGENTS},"sync":{}}),
    );
    assert_eq!(cold["entries"].as_array().map(Vec::len), Some(HOST_AGENTS));
    for entry in cold["entries"].as_array().expect("cold entries") {
        assert_child_control_flags(&entry["agent"]);
    }
    let mut changed = agent(7);
    changed.title = Some("Renamed while the client was away".to_owned());
    agents.upsert(&changed).expect("change one agent");
    let resumed = synchronized(
        &directory,
        json!({"scope":"active","sort":sort,"page":{"limit":HOST_AGENTS},
            "sync":{"generation":cold["sync"]["generation"],"afterSeq":cold["sync"]["headSeq"]}}),
    );
    let get = execute(
        &mut directory,
        "agent.get.request",
        json!({"agentId":"agent-007"}),
    )
    .expect("agent get");
    assert_eq!(
        page["entries"].as_array().map(Vec::len),
        Some(WORKSPACE_AGENTS)
    );
    assert_eq!(resumed["entries"].as_array().map(Vec::len), Some(1));
    vec![
        Measured::new("agent directory: workspace page", &page, "entries"),
        Measured::new("agent directory: synchronized cold", &cold, "entries"),
        Measured::new("agent directory: synchronized resume", &resumed, "entries"),
        Measured::new("agent directory: get", &get, "agent"),
    ]
}

/// Answer a synchronized `agent.list.request` as the execution worker does for idle Agents.
fn synchronized(directory: &AgentRuntimeDirectory, params: Value) -> Value {
    let request: AgentListRequest = serde_json::from_value(params).expect("list request");
    let cursor = request.sync.clone().unwrap_or_default();
    let value = sync_snapshot(directory, request).expect("synchronized snapshot");
    synchronize(directory, value, &cursor, false).expect("synchronized read")
}

fn agent(index: usize) -> PersistedAgentRuntimeRecord {
    let workspace = index % (HOST_AGENTS / WORKSPACE_AGENTS);
    serde_json::from_value(json!({
        "id":format!("agent-{index:03}"),"provider":"codex",
        "cwd":format!("/repo/.worktrees/{workspace}"),"workspaceId":format!("wks-{workspace}"),
        "createdAt":"2026-11-20T09:00:00.000Z",
        "updatedAt":format!("2026-11-20T10:{:02}:{:02}.000Z", index / 60, index % 60),
        "lastActivityAt":"2026-11-20T10:00:00.000Z","lastUserMessageAt":"2026-11-20T10:00:00.000Z",
        "title":format!("Investigate flaky timeline pagination test {index}"),
        "titleOrigin":"generated","labels":{"team":"server"},
        "lastStatus":"idle","lastModeId":"auto",
        "config":{"modeId":"auto","model":"gpt-5.1-codex","thinkingOptionId":"high"},
        "runtimeInfo":{"provider":"codex","sessionId":format!("019a0000-0000-7000-8000-{index:012}"),
            "model":"gpt-5.1-codex","thinkingOptionId":"high","modeId":"auto"},
        "features":[],
        "persistence":{"provider":"codex","sessionId":format!("019a0000-0000-7000-8000-{index:012}")},
        "lastError":null,"requiresAttention":false,"attentionReason":null,
        "attentionTimestamp":null,"internal":false,"archivedAt":null
    }))
    .expect("decode the agent")
}

fn timeline_reads(scenario: &str, items: &[NativeItem]) -> Vec<Measured> {
    let timeline = Timeline::memory().expect("timeline");
    let record = agent(0);
    timeline
        .append(&record.id, &record.provider, items)
        .expect("append the history");
    let mut envelope = serde_json::to_value(snapshot(&record)).expect("agent snapshot");
    assert_child_control_flags(&envelope);
    envelope["lastError"] = json!(record.last_error);
    let read = |mut params: Value, history_needs_refresh: Option<bool>| {
        params["agentId"] = json!(record.id);
        params["limit"] = json!(PAGE);
        params["projection"] = json!("projected");
        let request: FetchRequest = serde_json::from_value(params).expect("timeline request");
        fetch_stored(
            &request,
            &timeline,
            &record.id,
            &envelope,
            history_needs_refresh,
        )
        .expect("timeline page")
    };

    let tail = read(json!({"direction":"tail","allowCached":true}), Some(false));
    let older = read(
        json!({"direction":"before","cursor":tail["startCursor"]}),
        None,
    );
    let head = tail["window"]["maxSeq"].as_u64().expect("head sequence");
    let catch_up = read(
        json!({"direction":"after","cursor":{"epoch":tail["epoch"],"seq":head - MISSED_ITEMS}}),
        None,
    );
    let replaced = read(
        json!({"direction":"after","cursor":{"epoch":"retired-epoch","seq":head}}),
        None,
    );
    assert_eq!(replaced["reset"], true);
    for page in [&tail, &older, &catch_up, &replaced] {
        assert!(!page["entries"].as_array().expect("entries").is_empty());
    }
    vec![
        Measured::new(format!("{scenario}: timeline tail"), &tail, "entries"),
        Measured::new(format!("{scenario}: older page"), &older, "entries"),
        Measured::new(
            format!("{scenario}: catch-up after reconnect"),
            &catch_up,
            "entries",
        ),
        Measured::new(
            format!("{scenario}: catch-up after epoch change"),
            &replaced,
            "entries",
        ),
    ]
}

fn assert_child_control_flags(agent: &Value) {
    assert_eq!(agent["capabilities"]["supportsChildGuide"], false);
    assert_eq!(agent["capabilities"]["supportsChildStop"], false);
}

/// One running turn: a prompt, then reasoning, tool calls and progress messages, where every
/// fiftieth tool call returns a large output.
fn long_turn() -> Vec<NativeItem> {
    let turn = "turn-long";
    let mut items = Vec::with_capacity(LONG_TURN_ITEMS);
    items.push(item(
        turn,
        0,
        json!({"type":"user_message","text":filler(2048)}),
    ));
    items.extend((1..LONG_TURN_ITEMS).map(|index| {
        let payload = match index % 10 {
            0 => json!({"type":"assistant_message","messageId":format!("message-{index}"),
                "text":filler(1024)}),
            1 => json!({"type":"reasoning","text":filler(600)}),
            _ if index % 50 == 5 => tool(index, LARGE_OUTPUT),
            _ => tool(index, SMALL_OUTPUT),
        };
        item(turn, index, payload)
    }));
    items
}

/// Finished turns of a prompt, reasoning, eight tool calls, a TODO snapshot and an answer.
fn finished_turns() -> Vec<NativeItem> {
    (0..FINISHED_TURNS)
        .flat_map(|turn_index| {
            let turn = format!("turn-{turn_index}");
            let base = turn_index * 12;
            let todos: Vec<Value> = (0..6)
                .map(|task| {
                    json!({"text":format!("Step {task} of the change in turn {turn_index}"),
                        "completed":task <= turn_index % 6})
                })
                .collect();
            let mut items = vec![
                item(
                    &turn,
                    base,
                    json!({"type":"user_message","text":filler(300)}),
                ),
                item(
                    &turn,
                    base + 1,
                    json!({"type":"reasoning","text":filler(400)}),
                ),
            ];
            items.extend(
                (2..10).map(|offset| item(&turn, base + offset, tool(base + offset, 3072))),
            );
            items.push(item(&turn, base + 10, json!({"type":"todo","items":todos})));
            items.push(item(
                &turn,
                base + 11,
                json!({"type":"assistant_message","messageId":format!("answer-{turn_index}"),
                    "text":filler(1536)}),
            ));
            items
        })
        .collect()
}

fn item(turn: &str, index: usize, payload: Value) -> NativeItem {
    NativeItem {
        key: format!("{turn}:{index}"),
        turn_id: Some(turn.to_owned()),
        timestamp: format!("2026-11-20T10:{:02}:{:02}Z", (index / 60) % 60, index % 60),
        item: payload,
    }
}

fn tool(index: usize, output_bytes: usize) -> Value {
    json!({"type":"tool_call","callId":format!("call-{index}"),"name":"shell",
        "status":"completed","error":null,"detail":{"type":"shell",
        "command":"cargo test -p provider --lib timeline","cwd":"/repo",
        "output":filler(output_bytes),"exitCode":0}})
}

fn filler(bytes: usize) -> String {
    FILLER.repeat(bytes / FILLER.len() + 1)[..bytes].to_owned()
}
