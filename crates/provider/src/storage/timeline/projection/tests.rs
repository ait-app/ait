use serde_json::{Value, json};

use crate::protocol::timeline::{FetchRequest, NativeItem};
use crate::rpc::timeline::{fetch, fetch_stored};

use super::*;

fn item(seq: u64, item: Value) -> NativeItem {
    NativeItem {
        key: format!("native:{seq}"),
        turn_id: Some("turn".to_owned()),
        timestamp: "2026-10-11T00:00:00Z".to_owned(),
        item,
    }
}

fn compare(timeline: &Timeline) {
    let (epoch, rows) = timeline.read("agent").unwrap();
    for direction in ["tail", "before", "after"] {
        for limit in [0, 1, 3, 40] {
            for cursor in [0, 1, 2, 4, rows.last().map_or(0, |row| row.seq)] {
                let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
                    "direction":direction,"limit":limit,"cursor":{"epoch":epoch,"seq":cursor}}))
                .unwrap();
                assert_eq!(
                    fetch_stored(&request, timeline, "agent", &Value::Null),
                    fetch(&request, &epoch, &rows, &Value::Null),
                    "{direction}, {limit}, {cursor}"
                );
            }
        }
    }
}

#[test]
fn incremental_groups_match_the_reference_through_progress_completion_and_replacement() {
    let timeline = Timeline::memory().unwrap();
    assert!(!timeline.has_entries("agent").unwrap());
    let entries = vec![
        item(
            1,
            json!({"type":"tool_call","callId":"one","status":"running"}),
        ),
        item(
            2,
            json!({"type":"assistant_message","text":"A","messageId":"message"}),
        ),
        item(3, json!({"type":"assistant_message","text":"B"})),
        item(
            4,
            json!({"type":"tool_call","callId":"two","status":"running"}),
        ),
        item(
            5,
            json!({"type":"tool_call","callId":"one","status":"completed","detail":{"type":"unknown","output":"done"}}),
        ),
        item(6, json!({"type":"reasoning","text":"R"})),
        item(7, json!({"type":"reasoning","text":"S"})),
        item(
            8,
            json!({"type":"tool_call","callId":"two","status":"failed","error":"failed"}),
        ),
    ];
    for entry in &entries {
        timeline
            .append("agent", "codex", std::slice::from_ref(entry))
            .unwrap();
        compare(&timeline);
    }
    assert!(timeline.has_entries("agent").unwrap());
    let text = item(9, json!({"type":"assistant_message","text":"partial"}));
    timeline
        .progress("agent", "codex", "observation", &text)
        .unwrap();
    compare(&timeline);
    timeline
        .append(
            "agent",
            "codex",
            &[NativeItem {
                item: json!({"type":"assistant_message","text":"partial final"}),
                ..text
            }],
        )
        .unwrap();
    compare(&timeline);
    timeline.reconcile("agent", "codex", &entries[..4]).unwrap();
    compare(&timeline);
    let stale: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
        "direction":"after","cursor":{"epoch":"old","seq":1},"limit":2}))
    .unwrap();
    let (epoch, rows) = timeline.read("agent").unwrap();
    assert_eq!(
        fetch_stored(&stale, &timeline, "agent", &Value::Null),
        fetch(&stale, &epoch, &rows, &Value::Null)
    );
}

#[test]
fn durable_index_reads_only_the_requested_window_after_reopening() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).unwrap();
    let entries = (1..=5000)
        .map(|seq| item(seq, json!({"type":"user_message","text":seq.to_string()})))
        .collect::<Vec<_>>();
    timeline.append("agent", "codex", &entries).unwrap();
    let epoch = timeline.generation("agent").unwrap();
    {
        let database = timeline.database.lock().unwrap();
        database
            .execute(
                "UPDATE entries SET entry='invalid old source' WHERE seq<4900",
                [],
            )
            .unwrap();
        database
            .execute(
                "UPDATE display_sources SET entry='invalid old source' WHERE seq<4900",
                [],
            )
            .unwrap();
        database
            .execute(
                "UPDATE display_entries SET entry='invalid old projection' WHERE seq_start<4900",
                [],
            )
            .unwrap();
    }
    drop(timeline);
    let reopened = Timeline::open(&path).unwrap();
    for (direction, cursor, expected) in [
        ("tail", 5000, 4998),
        ("before", 4999, 4996),
        ("after", 4997, 4998),
    ] {
        let request: FetchRequest =
            serde_json::from_value(json!({"agentId":"agent","direction":direction,
            "limit":3,"cursor":{"epoch":epoch,"seq":cursor}}))
            .unwrap();
        let response = fetch_stored(&request, &reopened, "agent", &Value::Null).unwrap();
        assert_eq!(response["entries"][0]["seqStart"], expected);
        assert_eq!(response["entries"].as_array().unwrap().len(), 3);
        assert_eq!(response["window"]["maxSeq"], 5000);
    }
}

#[test]
fn byte_fallback_preserves_large_text_and_interleaved_tool_payloads() {
    for kind in ["reasoning", "tool_call"] {
        let timeline = Timeline::memory().unwrap();
        let entries = (1..=4).map(|seq| {
            let value = if kind == "reasoning" { json!({"type":kind,"text":"界".repeat(128*1024)}) }
                else { json!({"type":kind,"callId":if seq%2==1 {"one"} else {"two"},
                    "status":if seq<=2 {"running"} else {"completed"},
                    "detail":{"type":"unknown","output":if seq<=2 {String::new()} else {"x".repeat(700*1024)}}}) };
            item(seq,value)
        }).collect::<Vec<_>>();
        timeline.append("agent", "codex", &entries).unwrap();
        compare(&timeline);
    }
}

#[test]
fn version_five_builds_once_and_failed_appends_leave_the_index_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).unwrap();
    timeline
        .append(
            "agent",
            "codex",
            &[item(1, json!({"type":"user_message","text":"first"}))],
        )
        .unwrap();
    timeline
        .database
        .lock()
        .unwrap()
        .execute_batch(
            "DROP TABLE projection_heads; DROP TABLE display_entries; DROP TABLE display_sources;
         DROP TABLE display_ranges; PRAGMA user_version=5;",
        )
        .unwrap();
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();
    compare(&timeline);
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","limit":3})).unwrap();
    let before = fetch_stored(&request, &timeline, "agent", &Value::Null).unwrap();
    assert_eq!(
        timeline.append(
            "agent",
            "codex",
            &[
                item(2, json!({"type":"user_message","text":"new"})),
                item(1, json!({"type":"user_message","text":"conflict"})),
            ]
        ),
        Err(ErrorCode::IdempotencyConflict)
    );
    assert_eq!(
        fetch_stored(&request, &timeline, "agent", &Value::Null).unwrap(),
        before
    );
}
