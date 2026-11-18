use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use domain::agent_runtime::registry::AgentRuntimeRegistryError;
use domain::agent_runtime::{AgentRuntimeStatus, PersistedAgentRuntimeRecord};
use serde_json::Value;

use super::{AgentsStatus, Durability};

type Transform = Box<
    dyn FnOnce(
        &PersistedAgentRuntimeRecord,
    ) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeRegistryError>,
>;

fn record(id: &str) -> PersistedAgentRuntimeRecord {
    PersistedAgentRuntimeRecord {
        id: id.to_owned(),
        provider: "codex".to_owned(),
        cwd: "/tmp/project".to_owned(),
        workspace_id: None,
        created_at: "2026-09-20T10:00:00.000Z".to_owned(),
        updated_at: "2026-09-20T10:01:00.000Z".to_owned(),
        last_activity_at: None,
        last_user_message_at: None,
        title: None,
        title_origin: None,
        labels: BTreeMap::new(),
        last_status: AgentRuntimeStatus::Idle,
        last_mode_id: None,
        config: None,
        runtime_info: None,
        features: Vec::new(),
        persistence: None,
        last_error: None,
        requires_attention: false,
        attention_reason: None,
        attention_timestamp: None,
        internal: false,
        archived_at: None,
        owner: None,
        observation: None,
    }
}

fn titled(title: &str) -> Transform {
    let title = title.to_owned();
    Box::new(move |current| {
        let mut next = current.clone();
        next.title = Some(title);
        Ok(next)
    })
}

fn stored(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// A store whose flusher never fires during a test.
fn idle_store(path: &Path) -> Arc<AgentsStatus> {
    Arc::new(AgentsStatus::with_interval(
        path.to_owned(),
        Duration::from_secs(3600),
    ))
}

#[test]
fn deferred_update_is_read_at_once_and_written_only_on_flush() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("agent")).unwrap();
    let written_at = stored(&path)["lastChangedAt"].clone();

    status
        .update("agent", titled("deferred"), Durability::Deferred)
        .unwrap();

    let cached = status.get_shared("agent").unwrap().unwrap();
    assert_eq!(cached.title.as_deref(), Some("deferred"));
    assert!(stored(&path)["agents"][0]["title"].is_null());
    status.flush().unwrap();
    let flushed = stored(&path);
    assert_eq!(flushed["agents"][0]["title"], "deferred");
    assert_eq!(flushed["closed"], false);
    assert!(flushed["lastChangedAt"].as_str().unwrap() >= written_at.as_str().unwrap());
}

#[test]
fn immediate_write_includes_pending_status_and_clean_flush_writes_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("first")).unwrap();
    status
        .update("first", titled("pending"), Durability::Deferred)
        .unwrap();

    status.upsert(record("second")).unwrap();

    assert_eq!(stored(&path)["agents"][0]["title"], "pending");
    std::fs::remove_file(&path).unwrap();
    status.flush().unwrap();
    assert!(!path.exists(), "a clean document must not be rewritten");
}

#[test]
fn dropping_the_last_owner_writes_deferred_status() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("agent")).unwrap();
    status
        .update("agent", titled("on drop"), Durability::Deferred)
        .unwrap();

    drop(status);

    let reopened = AgentsStatus::new(path);
    let agent = reopened.get_shared("agent").unwrap().unwrap();
    assert_eq!(agent.title.as_deref(), Some("on drop"));
}

#[test]
fn flusher_writes_deferred_status_on_its_interval() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = Arc::new(AgentsStatus::with_interval(
        path.clone(),
        Duration::from_millis(20),
    ));
    status.upsert(record("agent")).unwrap();

    status
        .update("agent", titled("timer"), Durability::Deferred)
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while stored(&path)["agents"][0]["title"] != "timer" {
        assert!(Instant::now() < deadline, "the flusher did not write");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn close_marks_the_file_clean_and_rejects_later_writes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = Arc::new(AgentsStatus::with_interval(
        path.clone(),
        Duration::from_millis(10),
    ));
    status.upsert(record("agent")).unwrap();
    status
        .update("agent", titled("closing"), Durability::Deferred)
        .unwrap();

    status.close().unwrap();
    let closed = std::fs::read(&path).unwrap();

    assert_eq!(stored(&path)["closed"], true);
    assert_eq!(stored(&path)["agents"][0]["title"], "closing");
    assert_eq!(
        status.update("agent", titled("late"), Durability::Deferred),
        Err(AgentRuntimeRegistryError::Io)
    );
    assert_eq!(
        status.upsert(record("late")),
        Err(AgentRuntimeRegistryError::Io)
    );
    assert_eq!(status.remove("agent"), Err(AgentRuntimeRegistryError::Io));
    status.close().unwrap();
    std::thread::sleep(Duration::from_millis(50));
    drop(status);
    assert_eq!(std::fs::read(&path).unwrap(), closed);
    assert_eq!(AgentsStatus::new(path).interrupted_since().unwrap(), None);
}

#[test]
fn an_unclosed_file_reports_when_it_last_changed() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("agent")).unwrap();
    let changed_at = stored(&path)["lastChangedAt"].as_str().unwrap().to_owned();

    let reopened = AgentsStatus::new(path.clone());

    assert_eq!(reopened.interrupted_since().unwrap(), Some(changed_at));
    assert_eq!(status.interrupted_since().unwrap(), None);
}

#[test]
fn legacy_array_counts_as_closed_and_is_rewritten_as_a_document() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    std::fs::write(&path, serde_json::to_vec(&vec![record("legacy")]).unwrap()).unwrap();
    let status = idle_store(&path);

    assert_eq!(status.interrupted_since().unwrap(), None);
    status
        .update("legacy", titled("upgraded"), Durability::Immediate)
        .unwrap();

    let document = stored(&path);
    assert_eq!(document["agents"][0]["title"], "upgraded");
    assert!(document["lastChangedAt"].is_string());
}

#[test]
fn failed_immediate_writes_leave_the_cache_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("kept")).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();

    assert_eq!(
        status.upsert(record("rejected")),
        Err(AgentRuntimeRegistryError::Io)
    );
    assert_eq!(
        status.update("kept", titled("rejected"), Durability::Immediate),
        Err(AgentRuntimeRegistryError::Io)
    );
    assert_eq!(status.remove("kept"), Err(AgentRuntimeRegistryError::Io));

    assert!(status.get_shared("rejected").unwrap().is_none());
    let kept = status.get_shared("kept").unwrap().unwrap();
    assert_eq!(kept.title, None);
    assert_eq!(status.snapshot().unwrap().len(), 1);
    std::fs::remove_dir(&path).unwrap();
    assert!(status.remove("kept").unwrap());
    assert_eq!(stored(&path)["agents"], serde_json::json!([]));
}

#[test]
fn failed_flush_keeps_status_pending() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("agent")).unwrap();
    status
        .update("agent", titled("retry"), Durability::Deferred)
        .unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();

    assert_eq!(status.flush(), Err(AgentRuntimeRegistryError::Io));
    std::fs::remove_dir(&path).unwrap();
    status.flush().unwrap();

    assert_eq!(stored(&path)["agents"][0]["title"], "retry");
}

#[test]
fn oversized_deferred_changes_are_rejected_before_commit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let status = idle_store(&path);
    status.upsert(record("agent")).unwrap();
    let huge = "x".repeat(super::max_document_bytes());

    assert_eq!(
        status.update("agent", titled(&huge), Durability::Deferred),
        Err(AgentRuntimeRegistryError::InvalidRecord)
    );

    assert_eq!(status.get_shared("agent").unwrap().unwrap().title, None);
    status.upsert(record("other")).unwrap();
    assert_eq!(stored(&path)["agents"].as_array().unwrap().len(), 2);
}

#[test]
fn malformed_documents_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    std::fs::write(&path, br#"{"lastChangedAt":null}"#).unwrap();

    assert_eq!(
        AgentsStatus::new(path).initialize(),
        Err(AgentRuntimeRegistryError::InvalidFile)
    );
}

#[test]
fn first_deferred_change_after_a_clean_close_reopens_the_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("agents.json");
    let first = idle_store(&path);
    first.upsert(record("agent")).unwrap();
    first.close().unwrap();
    let restarted = idle_store(&path);

    restarted
        .update("agent", titled("after restart"), Durability::Deferred)
        .unwrap();
    restarted
        .update("agent", titled("still deferred"), Durability::Deferred)
        .unwrap();

    let document = stored(&path);
    assert_eq!(document["closed"], false);
    assert_eq!(document["agents"][0]["title"], "after restart");
    assert!(
        AgentsStatus::new(path)
            .interrupted_since()
            .unwrap()
            .is_some(),
        "a crash now would trigger recovery"
    );
}
