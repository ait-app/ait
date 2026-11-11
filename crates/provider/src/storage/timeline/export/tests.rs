use super::*;
use serde_json::json;

fn item(index: usize, kind: &str, text: &str) -> NativeItem {
    NativeItem {
        key: format!("native:{index}"),
        turn_id: Some("turn".to_owned()),
        timestamp: "2026-12-01T00:00:00Z".to_owned(),
        item: json!({"type":kind,"text":text}),
    }
}

#[test]
fn export_pages_past_visible_history_and_does_not_create_missing_state() {
    let timeline = Timeline::memory().unwrap();
    let before = timeline.database.lock().unwrap().total_changes();
    assert_eq!(
        timeline
            .export_rows("missing", &CancellationToken::new())
            .unwrap(),
        Vec::<Row>::new()
    );
    assert_eq!(timeline.database.lock().unwrap().total_changes(), before);
    let items: Vec<_> = (0..(PAGE_ROWS * 2 + 7))
        .map(|index| item(index, "user_message", &format!("message {index}")))
        .collect();
    timeline.append("agent", "codex", &items).unwrap();
    timeline
        .append(
            "other",
            "codex",
            &[item(999, "user_message", "not this agent")],
        )
        .unwrap();
    let before = timeline.database.lock().unwrap().total_changes();
    let rows = timeline
        .export_rows("agent", &CancellationToken::new())
        .unwrap();
    assert_eq!(rows.len(), items.len());
    assert_eq!(rows.last().unwrap().entry, *items.last().unwrap());
    assert_eq!(timeline.database.lock().unwrap().total_changes(), before);
}

#[test]
fn progress_completion_normalization_survives_page_boundaries() {
    let timeline = Timeline::memory().unwrap();
    let prefix = item(900, "assistant_message", "first ");
    timeline
        .progress("agent", "codex", "part", &prefix)
        .unwrap();
    let middle: Vec<_> = (0..PAGE_ROWS)
        .map(|index| item(index, "user_message", "middle"))
        .collect();
    timeline.append("agent", "codex", &middle).unwrap();
    let complete = item(900, "assistant_message", "first final");
    timeline.append("agent", "codex", &[complete]).unwrap();
    let rows = timeline
        .export_rows("agent", &CancellationToken::new())
        .unwrap();
    assert_eq!(rows.last().unwrap().entry.item["text"], "final");
    assert_eq!(rows, timeline.read("agent").unwrap().1);
}

#[test]
fn cancellation_and_complete_source_bounds_return_errors_not_prefixes() {
    let timeline = Timeline::memory().unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        timeline.export_rows("agent", &cancelled),
        Err(ErrorCode::AgentIo)
    );
    {
        let mut database = timeline.database.lock().unwrap();
        let transaction = database.transaction().unwrap();
        let encoded = serde_json::to_string(&item(0, "user_message", "saved")).unwrap();
        for index in 0..=MAX_SOURCE_ROWS {
            transaction
                .execute(
                    "INSERT INTO entries VALUES('agent',?,?, 'codex',?)",
                    params![index + 1, index.to_string(), encoded],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    assert_eq!(
        timeline.export_rows("agent", &CancellationToken::new()),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn oversized_saved_source_fails_before_returning_partial_content() {
    let timeline = Timeline::memory().unwrap();
    let encoded = serde_json::to_string(&item(0, "user_message", &"x".repeat(600_000))).unwrap();
    {
        let mut database = timeline.database.lock().unwrap();
        let transaction = database.transaction().unwrap();
        for index in 1..=29 {
            transaction
                .execute(
                    "INSERT INTO entries VALUES('agent',?,?, 'codex',?)",
                    params![index, index.to_string(), encoded],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
    }
    assert_eq!(
        timeline.export_rows("agent", &CancellationToken::new()),
        Err(ErrorCode::ResourceExhausted)
    );
}
