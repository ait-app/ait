//! Recorded outputs from the real Paseo projection and paging test suite.

use super::page::select;
use super::*;
use crate::protocol::timeline::{Direction, NativeItem};

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
