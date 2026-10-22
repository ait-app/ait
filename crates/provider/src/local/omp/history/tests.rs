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
