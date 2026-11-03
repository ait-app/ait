//! Recorded outputs from the real Paseo projection and paging test suite.

use super::page::select;
use super::*;
use crate::protocol::timeline::{Direction, NativeItem};

#[test]
fn complete_process_arguments_replace_a_partial_file_card_in_the_same_lifecycle() {
    for field in ["path", "file_path", "filePath"] {
        let mut input = json!({"content":null});
        input[field] = json!("proc://collector/kill");
        let completed = json!({"type":"tool_call","callId":"process","name":"write",
            "status":"completed","error":null,"detail":{"type":"unknown",
            "input":input,"output":"Stopped"}});
        let source = rows(&json!({"rows":[
            {"seq":1,"turnId":"turn","timestamp":"2026-11-03T00:00:00Z",
                "item":{"type":"tool_call","callId":"process","name":"write",
                "status":"running","error":null,"detail":{"type":"write","filePath":"proc"}}},
            {"seq":2,"turnId":"turn","timestamp":"2026-11-03T00:00:01Z","item":completed}
        ]}));
        let projected = project(&source);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].item, completed);
        assert_eq!(
            projected[0].source_seq_ranges,
            [Range {
                start_seq: 1,
                end_seq: 2
            }]
        );
        assert_eq!(source[0].entry.item["detail"]["type"], "write");
    }
}

#[test]
fn legacy_tool_details_match_the_strict_protocol_fixtures() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../tests/fixtures/legacy_tool_details.json"
    ))
    .unwrap();
    for case in cases {
        assert_eq!(
            normalize_item(case["item"].clone()),
            case["expected"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn legacy_null_write_is_normalized_in_source_and_cached_views_only() {
    for detail in [
        json!({"type":"write","filePath":"file.rs","content":null}),
        json!({"type":"write","filePath":"proc://collector/kill","content":null}),
        json!({"type":"shell","command":"pwd","output":null,"cwd":null}),
    ] {
        let source = rows(&json!({"rows":[{"seq":319,"turnId":"turn",
            "timestamp":"2026-11-03T00:00:00Z","item":{"type":"tool_call",
            "callId":"call","name":"write","status":"completed","detail":detail}}]}));
        let raw = source[0].entry.item.clone();
        let projected = Entry::from(&source[0]);
        let mut cached = serde_json::to_value(&projected).unwrap();
        cached["item"] = raw.clone();
        let decoded: Entry = serde_json::from_value(cached).unwrap();
        assert_eq!(decoded, projected);
        assert_eq!(source[0].entry.item, raw);
        assert_eq!(projected.seq_start, 319);
        assert_eq!(projected.seq_end, 319);
        assert_eq!(
            projected.source_seq_ranges,
            [Range {
                start_seq: 319,
                end_seq: 319
            }]
        );
        if detail["filePath"] == "proc://collector/kill" {
            assert_eq!(
                projected.item["detail"],
                json!({"type":"unknown","input":detail,"output":null})
            );
        } else {
            assert!(projected.item["detail"].get("content").is_none());
            assert!(projected.item["detail"].get("output").is_none());
            assert!(projected.item["detail"].get("cwd").is_none());
        }
    }
}

#[test]
fn history_compatibility_preserves_strings_and_does_not_coerce_invalid_values() {
    for content in [json!(""), json!("text"), json!({"invalid":true})] {
        let item = json!({"type":"tool_call","detail":{"type":"write","filePath":"file.rs","content":content}});
        assert_eq!(normalize_item(item.clone()), item);
    }
    let unrelated = json!({"type":"plugin","detail":{"type":"write","content":null}});
    assert_eq!(normalize_item(unrelated.clone()), unrelated);
}

#[test]
fn shell_compatibility_preserves_valid_output_errors_and_unknown_tool_payloads() {
    for item in [
        json!({"type":"tool_call","status":"completed","error":null,
            "detail":{"type":"shell","command":"true","cwd":"/workspace","output":""}}),
        json!({"type":"tool_call","status":"failed","error":{"message":"denied"},
            "detail":{"type":"unknown","input":null,"output":null}}),
        json!({"type":"assistant_message","text":"unchanged"}),
    ] {
        assert_eq!(normalize_item(item.clone()), item);
    }
}

fn rows(input: &Value) -> Vec<Row> {
    input["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| Row {
            seq: value["seq"].as_u64().unwrap(),
            provider: "codex".to_owned(),
            entry: NativeItem {
                key: value["seq"].to_string(),
                turn_id: value["turnId"].as_str().map(str::to_owned),
                timestamp: value["timestamp"].as_str().unwrap().to_owned(),
                item: value["item"].clone(),
            },
        })
        .collect()
}

fn cases() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/paseo-timeline-projection.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["source"]["revision"],
        "30178c4f58b67f8472901356e1484022bd835de0"
    );
    fixture["cases"].as_array().unwrap().clone()
}

fn strip_provider(entries: &mut Value) {
    for entry in entries.as_array_mut().unwrap() {
        entry.as_object_mut().unwrap().remove("provider");
    }
}

#[test]
fn projection_matches_all_recorded_paseo_cases() {
    let mut count = 0;
    for case in cases()
        .into_iter()
        .filter(|case| case["operation"] == "projectTimelineRows")
    {
        let rows = rows(&case["input"]);
        let entries = if case["input"]["mode"] == "canonical" {
            rows.iter().map(Entry::from).collect()
        } else {
            project(&rows)
        };
        let mut actual = serde_json::to_value(entries).unwrap();
        strip_provider(&mut actual);
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 7);
}

#[test]
fn pagination_matches_all_recorded_paseo_cases() {
    let mut count = 0;
    for case in cases()
        .into_iter()
        .filter(|case| case["operation"] == "selectProjectedTimelinePage")
    {
        let input = &case["input"];
        let direction: Direction = serde_json::from_value(input["direction"].clone()).unwrap();
        let page = select(
            &rows(input),
            direction,
            input["cursorSeq"].as_u64(),
            usize::try_from(input["limit"].as_u64().unwrap_or(0)).unwrap(),
        );
        let mut actual = serde_json::to_value(page).unwrap();
        strip_provider(&mut actual["entries"]);
        assert_eq!(actual, case["expected"], "{}", case["name"]);
        count += 1;
    }
    assert_eq!(count, 11);
}
