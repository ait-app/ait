use super::*;

#[test]
fn task_snapshot_survives_tail_pagination_reopen_and_projection_rebuild() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).unwrap();
    let tasks = json!([{"id":"1","text":"Verify","completed":false,"status":"in_progress"}]);
    timeline
        .append(
            "agent",
            "codex",
            &[
                item(1, json!({"type":"todo","items":tasks})),
                item(2, json!({"type":"user_message","text":"Continue"})),
                item(3, json!({"type":"assistant_message","text":"Working"})),
            ],
        )
        .unwrap();
    timeline
        .append(
            "other-agent",
            "claude",
            &[item(1, json!({"type":"todo","items":[]}))],
        )
        .unwrap();
    let epoch = timeline.generation("agent").unwrap();
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();

    for rebuild in [false, true] {
        if rebuild {
            clear(&timeline.database.lock().unwrap(), "agent").unwrap();
        }
        for direction in ["tail", "before", "after"] {
            let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent",
                "direction":direction,"limit":1,"cursor":{"epoch":epoch,"seq":2}}))
            .unwrap();
            let page = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
            assert_eq!(
                page["taskSnapshot"],
                json!({"epoch":epoch,"seq":1,"items":tasks})
            );
            if direction == "tail" {
                assert_eq!(page["hasOlder"], true);
                assert_eq!(page["entries"][0]["item"]["type"], "assistant_message");
            }
        }
        compare(&timeline);
    }
}

#[test]
fn task_snapshot_distinguishes_absence_clear_and_replaced_history() {
    let timeline = Timeline::memory().unwrap();
    let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent"})).unwrap();
    let empty = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
    assert_eq!(empty["taskSnapshot"], Value::Null);
    timeline
        .append(
            "agent",
            "codex",
            &[
                item(
                    1,
                    json!({"type":"todo","items":[{"text":"Verify","completed":false}]}),
                ),
                item(2, json!({"type":"todo","items":[]})),
                item(3, json!({"type":"assistant_message","text":"Done"})),
            ],
        )
        .unwrap();
    let cleared = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
    assert_eq!(cleared["taskSnapshot"]["items"], json!([]));
    assert_eq!(cleared["taskSnapshot"]["seq"], 2);
    assert_eq!(cleared["taskSnapshot"]["epoch"], cleared["epoch"]);

    timeline
        .reconcile(
            "agent",
            "codex",
            &[item(4, json!({"type":"user_message","text":"Replacement"}))],
        )
        .unwrap();
    let replacement = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();
    assert_ne!(replacement["epoch"], cleared["epoch"]);
    assert_eq!(replacement["taskSnapshot"], Value::Null);
    compare(&timeline);
}

#[test]
fn task_snapshot_is_reserved_in_the_response_byte_budget() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "codex",
            &[
                item(
                    1,
                    json!({"type":"todo","items":[{"text":"T".repeat(200_000),"completed":false}]}),
                ),
                item(2, json!({"type":"user_message","text":"U".repeat(250_000)})),
                item(
                    3,
                    json!({"type":"assistant_message","text":"A".repeat(250_000)}),
                ),
                item(4, json!({"type":"user_message","text":"Z".repeat(250_000)})),
            ],
        )
        .unwrap();
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","limit":0})).unwrap();

    let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();

    assert_eq!(response["taskSnapshot"]["seq"], 1);
    assert_eq!(response["hasOlder"], true);
    assert!(serde_json::to_vec(&response).unwrap().len() <= 900 * 1024);
    compare(&timeline);
}

fn large_legacy_todo() -> NativeItem {
    item(
        1,
        json!({"type":"todo","items":vec![
            json!({"text":"T".repeat(6 * 1024),"completed":false});100
        ]}),
    )
}

#[test]
fn large_legacy_todo_uses_a_same_page_reference_instead_of_duplicate_items() {
    let timeline = Timeline::memory().unwrap();
    let todo = large_legacy_todo();
    timeline
        .append("agent", "claude", std::slice::from_ref(&todo))
        .unwrap();
    let epoch = timeline.generation("agent").unwrap();
    let request: FetchRequest = serde_json::from_value(json!({"agentId":"agent"})).unwrap();

    let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();

    assert_eq!(
        response["taskSnapshot"],
        json!({"epoch":epoch,"seq":1,"entrySeq":1})
    );
    assert_eq!(response["entries"][0]["item"], todo.item);
    assert!(serde_json::to_vec(&response).unwrap().len() <= 900 * 1024);
    let (_, rows) = timeline.read("agent").unwrap();
    assert_eq!(
        response,
        fetch(&request, &epoch, &rows, &Value::Null).unwrap()
    );
}

#[test]
fn large_current_todo_outside_the_latest_page_still_returns_its_complete_snapshot() {
    let timeline = Timeline::memory().unwrap();
    let todo = large_legacy_todo();
    timeline
        .append(
            "agent",
            "claude",
            &[
                todo.clone(),
                item(2, json!({"type":"assistant_message","text":"Continue"})),
            ],
        )
        .unwrap();
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","limit":1})).unwrap();

    let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();

    assert_eq!(response["taskSnapshot"]["items"], todo.item["items"]);
    assert_eq!(response["entries"][0]["seqStart"], 2);
    assert_eq!(response["hasOlder"], true);
    assert!(serde_json::to_vec(&response).unwrap().len() <= 900 * 1024);
}

#[test]
fn two_large_independent_items_keep_history_readable_without_a_false_empty_snapshot() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "claude",
            &[
                large_legacy_todo(),
                item(
                    2,
                    json!({"type":"assistant_message","text":"A".repeat(600 * 1024)}),
                ),
            ],
        )
        .unwrap();
    let request: FetchRequest =
        serde_json::from_value(json!({"agentId":"agent","limit":1})).unwrap();

    let response = fetch_stored(&request, &timeline, "agent", &Value::Null, None).unwrap();

    assert!(response.get("taskSnapshot").is_none());
    assert_eq!(response["entries"][0]["seqStart"], 2);
    assert_eq!(
        response["entries"][0]["item"]["text"]
            .as_str()
            .unwrap()
            .len(),
        600 * 1024
    );
    assert_eq!(response["hasOlder"], true);
    assert!(serde_json::to_vec(&response).unwrap().len() <= 900 * 1024);
}
