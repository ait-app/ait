use super::*;

fn record(status: &str, answer: &str, time: u64) -> Value {
    json!({"id":"topic","question":"Side question","answer":answer,"status":status,
        "createdAt":1,"updatedAt":time})
}

#[test]
fn deltas_stale_snapshots_and_terminal_records_preserve_the_latest_answer() {
    let mut state = State::default();
    let start = record("running", "", 1);
    state
        .capture(&json!({"type":"btw_record","record":start}))
        .unwrap();
    state
        .capture(&json!({"type":"btw_delta","recordId":"topic","delta":"partial"}))
        .unwrap();
    state
        .capture(
            &json!({"type":"response","command":"get_btw_history","success":true,
        "data":{"records":[start]}}),
        )
        .unwrap();
    assert_eq!(
        state.snapshot()["records"][0]["turns"][0]["answer"],
        "partial"
    );
    state
        .capture(&json!({"type":"btw_record","record":record("complete","final answer",2)}))
        .unwrap();
    state
        .capture(&json!({"type":"btw_record","record":record("running","",1)}))
        .unwrap();
    state
        .capture(&json!({"type":"btw_delta","recordId":"topic","delta":"late"}))
        .unwrap();
    assert!(!state.running());
    let snapshot = state.snapshot();
    assert_eq!(snapshot["records"][0]["turns"][0]["answer"], "final answer");
    assert_eq!(snapshot["records"][0]["turns"][0]["status"], "complete");
}

#[test]
fn a_followup_can_start_in_the_same_millisecond_and_restore_without_a_live_writer() {
    let mut state = State::default();
    let mut previous = record("complete", "previous", 1);
    state.restore(&json!({"records":[previous]})).unwrap();
    assert!(!state.running());
    previous["followUps"] = json!([{"question":"Follow up","answer":"","status":"running","createdAt":1,"updatedAt":1}]);
    state
        .capture(&json!({"type":"btw_record","record":previous}))
        .unwrap();
    assert!(state.running());
    assert_eq!(
        state.snapshot()["records"][0]["turns"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut restored = State::default();
    restored.restore(&json!({"records":[previous]})).unwrap();
    assert!(!restored.running());
    assert!(restored.snapshot()["reveal"].is_null());
}

#[test]
fn native_history_and_unicode_output_are_bounded_and_failures_are_separate() {
    let mut state = State::default();
    let records: Vec<_> = (0..20)
        .map(|i| {
            let mut value = record("complete", &"界".repeat(50000), i + 1);
            value["id"] = json!(i.to_string());
            value
        })
        .collect();
    state.restore(&json!({"records":records})).unwrap();
    let snapshot = state.snapshot();
    assert_eq!(snapshot["records"].as_array().unwrap().len(), MAX_TOPICS);
    assert_eq!(snapshot["omitted"], 12);
    assert_eq!(snapshot["records"][0]["truncated"], true);
    assert!(snapshot.to_string().len() < 1024 * 1024);
    assert!(
        state
            .capture(&json!({"type":"btw_record","record":{"id":true}}))
            .is_err()
    );
    state.malformed();
    assert!(state.snapshot()["error"].is_string());
    assert_eq!(
        state.snapshot()["records"].as_array().unwrap().len(),
        MAX_TOPICS
    );
}

#[test]
fn cancellation_and_late_deltas_cannot_resurrect_a_topic() {
    let mut state = State::default();
    state
        .capture(&json!({"type":"btw_record","record":record("running","",1)}))
        .unwrap();
    state
        .capture(&json!({"type":"btw_record","record":record("cancelled","kept",2)}))
        .unwrap();
    state
        .capture(&json!({"type":"btw_delta","recordId":"topic","delta":"wrong"}))
        .unwrap();
    assert_eq!(state.snapshot()["records"][0]["turns"][0]["answer"], "kept");
    assert!(!state.running());
}
