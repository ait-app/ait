use super::*;

#[test]
fn title_slots_and_off_branch_usage_preserve_the_active_transcript() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("session.jsonl");
    let values = [
        json!({"type":"title","title":"Native title"}),
        json!({"type":"session","version":3,"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        record("first", None, "kept"),
        record("old", Some("first"), "abandoned"),
        json!({"type":"model_usage","id":"usage","parentId":"old","activeLeafId":"first","timestamp":"2026-10-21T00:00:01Z"}),
    ];
    let mut encoded = values
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    encoded.push('\n');
    std::fs::write(&path, encoded).unwrap();
    let journal = read(path).unwrap();
    let handle = AgentPersistenceHandle {
        provider: "omp".into(),
        session_id: "id".into(),
        native_handle: None,
        metadata: None,
    };
    let (history, _) = journal
        .project(&OmpClient::new("unused".into()), &handle)
        .unwrap();
    assert_eq!(history.descriptor.title.as_deref(), Some("Native title"));
    assert_eq!(history.entries.len(), 1);
}

fn record(id: &str, parent: Option<&str>, text: &str) -> Value {
    json!({"type":"message","id":id,"parentId":parent,"timestamp":"2026-10-21T00:00:00Z",
        "message":{"role":"user","timestamp":1000,"content":text}})
}

#[test]
fn only_active_branch_is_projected_and_corrupt_links_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let journal = Journal {
        path: root.path().join("history.jsonl"),
        header: json!({"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        records: vec![
            record("first", None, "one"),
            record("abandoned", Some("first"), "old"),
            record("current", Some("first"), "new"),
        ],
    };
    let client = OmpClient::new("missing".into());
    let handle = AgentPersistenceHandle {
        provider: "omp".into(),
        session_id: "id".into(),
        native_handle: None,
        metadata: None,
    };
    let (history, _) = journal.project(&client, &handle).unwrap();
    assert_eq!(history.entries.len(), 2);
    assert_eq!(history.entries[1].item["text"], "new");
    let corrupt = Journal {
        records: vec![record("self", Some("self"), "cycle")],
        ..journal
    };
    assert!(corrupt.branch().is_err());
}

#[test]
fn incomplete_records_and_invalid_headers_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("history.jsonl");
    std::fs::write(&path, "{\"type\":\"session\"}").unwrap();
    assert!(read(path.clone()).is_err());
    std::fs::write(&path, "{\"type\":\"session\",\"version\":2}\n").unwrap();
    assert!(read(path).is_err());
}

#[test]
fn background_results_have_the_same_tool_identity_in_live_output_and_history() {
    let root = tempfile::tempdir().unwrap();
    let result = json!({"type":"custom_message","id":"background","parentId":"user",
        "timestamp":"2026-10-21T00:00:01Z","customType":"async-result","display":true,
        "content":"Background job bg_4 has completed. Resume your work.\ncommand output",
        "details":{"jobs":[{"jobId":"bg_4","status":"completed","type":"bash"}]}});
    let journal = Journal {
        path: root.path().join("history.jsonl"),
        header: json!({"id":"id","cwd":"/tmp","timestamp":"2026-10-21T00:00:00Z"}),
        records: vec![record("user", None, "request"), result.clone()],
    };
    let handle = AgentPersistenceHandle {
        provider: "omp".into(),
        session_id: "id".into(),
        native_handle: None,
        metadata: None,
    };
    let (history, _) = journal
        .project(&OmpClient::new("missing".into()), &handle)
        .unwrap();
    let mut live = Stream::new("id".into(), crate::local::images::ImageStore::default());
    live.message(&record_message(&result).unwrap()).unwrap();
    let AgentTurnEvent::Timeline(entry) = live.events.pop_front().unwrap() else {
        panic!("expected background result");
    };
    assert_eq!(history.entries.len(), 2);
    assert_eq!(history.entries[1], entry);
    assert_eq!(entry.item["type"], "tool_call");
    assert_eq!(entry.item["callId"], "background:bg_4");
    assert!(entry.item.get("text").is_none());

    let timeline =
        crate::storage::timeline::Timeline::open(&root.path().join("timeline.sqlite")).unwrap();
    let mut legacy = history.entries.clone();
    legacy[1].item = json!({"type":"assistant_message","messageId":entry.key,
        "text":result["content"],"metadata":{"nativeRole":"custom","customType":"async-result"}});
    let (old_epoch, _) = timeline.append("agent", "omp", &legacy).unwrap();
    let replacement = timeline
        .reconcile("agent", "omp", &history.entries)
        .unwrap();
    assert_ne!(replacement, old_epoch);
    let (_, restored) = timeline.read("agent").unwrap();
    assert_eq!(restored[1].entry.item, entry.item);
}
