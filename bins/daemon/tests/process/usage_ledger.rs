//! Host usage ledger through the real daemon: startup recovery, replayed history and the WS read.

use std::time::Duration;

use serde_json::{Value, json};

use super::native::NativeFixture;
use super::transport::{Socket, connect, request};
use super::{ready, start, start_with_path, terminate};

const METHODS: &[&str] = &[
    "workspace.open.request",
    "agent.create.request",
    "agent.message.send.request",
    "agent.finish.wait.request",
    "provider.usage.ledger.request",
];

/// One UTC day in milliseconds.
const DAY_MS: i64 = 86_400_000;

/// Ledger read over the day around now, grouped by `group_by`; returns the rows and `since`.
async fn ledger(socket: &mut Socket, group_by: &[&str]) -> (Vec<Value>, i64) {
    let now = chrono::Utc::now().timestamp_millis();
    let response = request(
        socket,
        "provider.usage.ledger.request",
        json!({"from":now - DAY_MS,"to":now + DAY_MS,"groupBy":group_by}),
    )
    .await;
    assert_eq!(response["type"], "response", "{response}");
    let rows = response["result"]["rows"].as_array().unwrap().clone();
    (rows, response["result"]["since"].as_i64().unwrap())
}

/// The row of user calls with `outcome`, if any.
fn user_calls<'a>(rows: &'a [Value], outcome: &str) -> Option<&'a Value> {
    rows.iter()
        .find(|row| row["key"]["source"] == "user" && row["key"]["outcome"] == outcome)
}

/// Token counters of a row: requests, input, output and cache read.
fn tokens(row: &Value) -> [&Value; 4] {
    [
        &row["requests"],
        &row["inputTokens"],
        &row["outputTokens"],
        &row["cacheReadTokens"],
    ]
}

/// Send `text` to Agent `id` and return once the turn has started.
async fn send(socket: &mut Socket, id: &str, text: &str) {
    let sent = request(
        socket,
        "agent.message.send.request",
        json!({"agentId":id,"text":text,"messageId":text}),
    )
    .await;
    assert_eq!(sent["result"]["accepted"], true, "{sent}");
}

/// Wait until Agent `id` has finished its turn and is idle.
async fn finished(socket: &mut Socket, id: &str) {
    let outcome = request(
        socket,
        "agent.finish.wait.request",
        json!({"agentId":id,"timeoutMs":10000}),
    )
    .await;
    assert_eq!(outcome["result"]["status"], "idle", "{outcome}");
}

#[tokio::test]
async fn restart_interrupts_open_calls_and_replayed_history_is_counted_once() {
    let fixture = NativeFixture::new();
    let state = fixture.root.path().join("state");
    let log = fixture.root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    request(
        &mut socket,
        "workspace.open.request",
        json!({"cwd":fixture.cwd}),
    )
    .await;
    let created = request(
        &mut socket,
        "agent.create.request",
        json!({"config":{"provider":"codex","cwd":fixture.cwd}}),
    )
    .await;
    let id = created["result"]["agentId"].as_str().unwrap().to_owned();
    // The offline Codex reports two requests: 70 input, 7 output and 30 cache read tokens each.
    send(&mut socket, &id, "request-usage").await;
    finished(&mut socket, &id).await;
    send(&mut socket, &id, "hang").await;
    // A summary is answered after every write queued before it, so polling observes the start.
    let (rows, since) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (rows, since) = ledger(&mut socket, &["source", "outcome"]).await;
            if user_calls(&rows, "running").is_some() {
                return (rows, since);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the open call is recorded as running");
    assert_eq!(user_calls(&rows, "running").unwrap()["calls"], 1);
    assert_eq!(
        tokens(user_calls(&rows, "completed").unwrap()),
        [&json!(2), &json!(140), &json!(14), &json!(60)]
    );
    drop(socket);
    // A crashed host never finishes its open call.
    process.0.kill().unwrap();
    process.0.wait().unwrap();

    let mut process = start_with_path(&state, &log, Some(&fixture.path));
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    // Restart recovery resumes the saved Codex thread before this wait returns.
    finished(&mut socket, &id).await;
    send(&mut socket, &id, "after-restart").await;
    finished(&mut socket, &id).await;
    let (rows, restarted_since) = ledger(&mut socket, &["source", "outcome"]).await;

    assert_eq!(restarted_since, since);
    assert!(user_calls(&rows, "running").is_none(), "{rows:?}");
    let interrupted = user_calls(&rows, "interrupted").unwrap();
    assert_eq!(
        [&interrupted["calls"], &interrupted["missingCalls"]],
        [&json!(1), &json!(1)]
    );
    assert_eq!(
        tokens(user_calls(&rows, "completed").unwrap()),
        [&json!(2), &json!(140), &json!(14), &json!(60)]
    );
    terminate(&mut process).await;
}

#[tokio::test]
async fn an_unreadable_ledger_leaves_the_host_running_without_usage_history() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let file = state.join("agents/usage.sqlite3");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "not sqlite").unwrap();
    let mut process = start(&state, &log);
    let address = ready(&mut process, &log).await;
    let mut socket = connect(&address, METHODS).await;
    let now = chrono::Utc::now().timestamp_millis();

    let response = request(
        &mut socket,
        "provider.usage.ledger.request",
        json!({"from":now - DAY_MS,"to":now,"groupBy":["provider"]}),
    )
    .await;

    assert_eq!(response["code"], "unsupported_capability", "{response}");
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("usage ledger unavailable")
    );
    assert_eq!(std::fs::read(&file).unwrap(), b"not sqlite");
    terminate(&mut process).await;
}
