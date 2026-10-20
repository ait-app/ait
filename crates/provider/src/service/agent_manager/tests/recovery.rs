use super::*;
use crate::storage::timeline::Timeline;

#[test]
fn interrupted_turn_is_durable_idempotent_and_never_replayed() {
    let (manager, registry, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let manager = manager.with_timeline(timeline.clone());
    let mut original = stored_record();
    original.last_status = AgentRuntimeStatus::Running;
    original.last_user_message_at = Some(original.updated_at.clone());
    registry.upsert(&original).unwrap();

    manager.recover_turns().unwrap();
    let recovered = registry.get(&original.id).unwrap().unwrap();
    assert_eq!(recovered.last_status, AgentRuntimeStatus::Error);
    assert!(recovered.requires_attention);
    assert_eq!(
        recovered.attention_reason,
        Some(domain::agent_runtime::AgentAttentionReason::Error)
    );
    let message = recovered.last_error.as_deref().unwrap();
    assert!(message.contains("host restarted"));
    let mut expected = original;
    expected.last_status = recovered.last_status;
    expected.last_error.clone_from(&recovered.last_error);
    expected.attention_reason = recovered.attention_reason;
    expected
        .attention_timestamp
        .clone_from(&recovered.attention_timestamp);
    expected.updated_at.clone_from(&recovered.updated_at);
    expected
        .last_activity_at
        .clone_from(&recovered.last_activity_at);
    assert_eq!(recovered, expected);
    manager.recover_turns().unwrap();
    assert_eq!(registry.get(&recovered.id).unwrap().unwrap(), recovered);
    let rows = timeline.read(&recovered.id).unwrap().1;
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]
            .entry
            .key
            .starts_with("host:provider-failure:restart:")
    );
    assert_eq!(rows[0].entry.item["message"], message);
    let native = client.0.lock().unwrap();
    assert_eq!(native.create_calls, 0);
    assert_eq!(native.start_calls, 0);
    assert_eq!(native.resume_purposes, [] as [AgentResumePurpose; 0]);
}

#[test]
fn registry_failure_retries_without_losing_metadata_or_duplicating_the_notice() {
    let (manager, registry, _) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let manager = manager.with_timeline(timeline.clone());
    let mut record = stored_record();
    record.last_status = AgentRuntimeStatus::Running;
    registry.upsert(&record).unwrap();
    registry.0.lock().unwrap().fail_update = true;

    assert_eq!(manager.recover_turns(), Err(AgentManagerError::Registry));
    assert_eq!(registry.get(&record.id).unwrap().unwrap(), record);
    let notice = timeline.read(&record.id).unwrap().1;
    assert_eq!(notice.len(), 1);
    record.title = Some("Updated title".to_owned());
    record.labels.insert("keep".to_owned(), "yes".to_owned());
    record.updated_at = "2026-10-19T10:00:00Z".to_owned();
    registry.upsert(&record).unwrap();
    registry.0.lock().unwrap().fail_update = false;
    manager.recover_turns().unwrap();

    let recovered = registry.get(&record.id).unwrap().unwrap();
    assert_eq!(recovered.last_status, AgentRuntimeStatus::Error);
    assert_eq!(recovered.title, record.title);
    assert_eq!(recovered.labels, record.labels);
    assert_eq!(timeline.read(&record.id).unwrap().1, notice);
}

#[test]
fn timeline_failure_keeps_the_running_record_eligible_for_recovery() {
    let (manager, registry, _) = make_manager();
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("timeline.sqlite");
    let timeline = Timeline::open(&path).unwrap();
    let manager = manager.with_timeline(timeline.clone());
    let mut record = stored_record();
    record.last_status = AgentRuntimeStatus::Running;
    registry.upsert(&record).unwrap();
    let database = rusqlite::Connection::open(path).unwrap();
    database
        .execute_batch(
            "CREATE TRIGGER fail_notice BEFORE INSERT ON entries
        BEGIN SELECT RAISE(FAIL, 'fixture write failure'); END;",
        )
        .unwrap();

    assert_eq!(manager.recover_turns(), Err(AgentManagerError::Registry));
    assert_eq!(registry.get(&record.id).unwrap().unwrap(), record);
    assert_eq!(
        timeline.read(&record.id).unwrap().1,
        [] as [crate::storage::timeline::Row; 0]
    );
    database.execute_batch("DROP TRIGGER fail_notice").unwrap();
    manager.recover_turns().unwrap();
    assert_eq!(
        registry.get(&record.id).unwrap().unwrap().last_status,
        AgentRuntimeStatus::Error
    );
    assert_eq!(timeline.read(&record.id).unwrap().1.len(), 1);
}

#[test]
fn inactive_records_are_unchanged_but_unavailable_archived_writers_are_retired() {
    let (manager, registry, _) = make_manager();
    let timeline = Timeline::memory().unwrap();
    let manager = manager.with_timeline(timeline.clone());
    for status in [
        AgentRuntimeStatus::Idle,
        AgentRuntimeStatus::Error,
        AgentRuntimeStatus::Closed,
    ] {
        let mut record = stored_record();
        record.last_status = status;
        registry.upsert(&record).unwrap();
        manager.recover_turns().unwrap();
        assert_eq!(registry.get(&record.id).unwrap().unwrap(), record);
        assert!(!timeline.has_entries(&record.id).unwrap());
    }
    let mut record = stored_record();
    record.last_status = AgentRuntimeStatus::Running;
    record.provider = "unavailable".to_owned();
    record.archived_at = Some(record.updated_at.clone());
    registry.upsert(&record).unwrap();
    manager.recover_turns().unwrap();
    let recovered = registry.get(&record.id).unwrap().unwrap();
    assert_eq!(recovered.last_status, AgentRuntimeStatus::Error);
    assert_eq!(recovered.archived_at, record.archived_at);
}

#[tokio::test]
async fn a_live_writer_is_not_interrupted_by_recovery() {
    let (mut manager, registry, client) = make_manager();
    let timeline = Timeline::memory().unwrap();
    manager = manager.with_timeline(timeline.clone());
    manager
        .create("agent-1", &spec(), AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent-1", "work").await.unwrap();
    manager.poll().await.unwrap();
    let original = registry.get("agent-1").unwrap().unwrap();
    assert_eq!(original.last_status, AgentRuntimeStatus::Running);

    manager.recover_turns().unwrap();

    assert_eq!(registry.get("agent-1").unwrap().unwrap(), original);
    assert_eq!(manager.active_turn("agent-1"), Some("native-turn"));
    assert_eq!(client.0.lock().unwrap().close_calls, 0);
    assert!(!timeline.has_entries("agent-1").unwrap());
}
