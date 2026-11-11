use super::*;
use serde_json::json;

#[test]
fn version_six_migration_preserves_child_history_and_persists_host_bindings() {
    let root = tempfile::tempdir().expect("temporary directory");
    let path = root.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).expect("timeline");
    let child = NativeSubagent {
        persistence: None,
        id: "child".to_owned(),
        parent_id: "native-root".to_owned(),
        cwd: "/tmp".to_owned(),
        descriptor: json!({"id":"child","status":"completed"}),
    };
    timeline.store_subagent("root", &child).expect("descriptor");
    let entry = crate::protocol::timeline::NativeItem {
        key: "native:answer".to_owned(),
        turn_id: None,
        timestamp: "2026-10-31T00:00:00Z".to_owned(),
        item: json!({"type":"assistant_message","text":"Saved answer"}),
    };
    let initial = timeline
        .append("subagent:root:child", "codex", &[entry])
        .expect("history");
    drop(timeline);
    let database = rusqlite::Connection::open(&path).expect("legacy database");
    database.execute_batch("DROP INDEX provider_subagents_host; ALTER TABLE provider_subagents DROP COLUMN host_agent; ALTER TABLE provider_subagents DROP COLUMN host_observed; PRAGMA user_version=6;").expect("version six schema");
    drop(database);
    let timeline = Timeline::open(&path).expect("migrate");
    timeline
        .bind_subagent("root", "child", "observed", true)
        .expect("binding");
    assert_eq!(
        timeline.generation("subagent:root:child").expect("epoch"),
        initial.0
    );
    assert_eq!(
        timeline.read("subagent:root:child").expect("history").1[0].seq,
        initial.1[0]
    );
    assert_eq!(
        timeline.bind_subagent("root", "child", "different", true),
        Err(ErrorCode::IdempotencyConflict)
    );
    drop(timeline);
    assert_eq!(
        Timeline::open(&path)
            .expect("reopen")
            .subagent_agent("root", "child")
            .expect("binding"),
        Some("observed".to_owned())
    );
}

#[test]
fn bound_child_events_and_replacements_use_the_canonical_agent_identity() {
    let timeline = Timeline::memory().expect("timeline");
    let child = NativeSubagent {
        persistence: None,
        id: "child".to_owned(),
        parent_id: "native-root".to_owned(),
        cwd: "/tmp".to_owned(),
        descriptor: json!({"id":"child","status":"running"}),
    };
    timeline.store_subagent("root", &child).expect("descriptor");
    timeline
        .bind_subagent("root", "child", "observed", true)
        .expect("binding");
    let (outbound, mut receiver) = model::outbound::Outbound::new();
    let subscription = timeline.events().subscribe(
        "observer".to_owned(),
        std::collections::BTreeSet::from(["observed".to_owned()]),
        outbound,
        false,
    );
    subscription.activate().expect("subscribe");
    let mut entry = crate::protocol::timeline::NativeItem {
        key: "native:answer".to_owned(),
        turn_id: Some("turn".to_owned()),
        timestamp: "2026-10-31T00:00:00Z".to_owned(),
        item: json!({"type":"assistant_message","text":"Part"}),
    };
    timeline
        .progress("subagent:root:child", "codex", "delta", &entry)
        .expect("progress");
    entry.item["text"] = json!("Part complete");
    timeline
        .append("subagent:root:child", "codex", &[entry.clone()])
        .expect("completion");
    for seq in 1..=2 {
        let frame = receiver.try_recv().expect("child event");
        let model::outbound::Frame::Text(text) = &frame.message else {
            panic!("JSON event");
        };
        let value: serde_json::Value = serde_json::from_str(text).expect("event");
        assert_eq!(value["method"], "agent_stream");
        assert_eq!(value["params"]["agentId"], "observed");
        assert_eq!(value["params"]["seq"], seq);
    }
    entry.item["text"] = json!("Rewritten");
    timeline
        .reconcile("subagent:root:child", "codex", &[entry])
        .expect("replacement");
    let frame = receiver.try_recv().expect("replacement event");
    let model::outbound::Frame::Text(text) = &frame.message else {
        panic!("JSON event");
    };
    let value: serde_json::Value = serde_json::from_str(text).expect("event");
    assert_eq!(value["method"], "agent.timeline.replacement");
    assert_eq!(value["params"]["agentId"], "observed");
}

#[test]
fn native_task_state_survives_restart_without_reparenting_or_rewriting_its_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("timeline.db");
    let timeline = Timeline::open(&path).unwrap();
    let mut child = NativeSubagent {
        persistence: None,
        id: "child".into(),
        parent_id: "native-root".into(),
        cwd: "/tmp".into(),
        descriptor: json!({"id":"child","status":"running"}),
    };
    timeline.store_subagent("host", &child).unwrap();
    child.descriptor["status"] = json!("completed");
    timeline.store_subagent("host", &child).unwrap();
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();
    assert_eq!(timeline.subagents("host").unwrap(), vec![child.clone()]);
    assert_eq!(
        timeline.subagents("other").unwrap(),
        [] as [NativeSubagent; 0]
    );
    child.parent_id = "foreign".into();
    assert_eq!(
        timeline.store_subagent("host", &child),
        Err(ErrorCode::IdempotencyConflict)
    );
    assert_eq!(
        timeline.subagents("host").unwrap()[0].parent_id,
        "native-root"
    );
}
