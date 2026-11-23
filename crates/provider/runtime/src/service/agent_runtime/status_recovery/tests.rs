use std::collections::BTreeMap;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::agent_runtime::{AgentRuntimeStatus, PersistedAgentRuntimeRecord};
use model::ErrorCode;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use provider_core::protocol::timeline::NativeItem;
use serde_json::json;

use super::recover_status;
use crate::storage::timeline::Timeline;

const SINCE: &str = "2026-11-18T10:00:00.000Z";

fn record(id: &str, last_user_message_at: Option<&str>) -> PersistedAgentRuntimeRecord {
    PersistedAgentRuntimeRecord {
        model_preset: None,
        config_revision: 0,
        id: id.to_owned(),
        provider: "claude".to_owned(),
        cwd: "/repo".to_owned(),
        workspace_id: None,
        created_at: "2026-11-18T09:00:00.000Z".to_owned(),
        updated_at: "2026-11-18T09:59:00.000Z".to_owned(),
        last_activity_at: Some("2026-11-18T10:03:00.000Z".to_owned()),
        last_user_message_at: last_user_message_at.map(str::to_owned),
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

fn user_message(timeline: &Timeline, agent: &str, timestamp: &str) {
    let entry = NativeItem {
        key: format!("{agent}:{timestamp}"),
        turn_id: None,
        timestamp: timestamp.to_owned(),
        item: json!({"type":"user_message","text":"hello"}),
    };
    timeline.append(agent, "claude", &[entry]).unwrap();
}

fn fixture() -> (tempfile::TempDir, FileBackedAgentRuntimeRegistry, Timeline) {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    (root, registry, Timeline::memory().unwrap())
}

#[test]
fn restores_input_timestamps_lost_after_the_last_write() {
    let (_root, registry, timeline) = fixture();
    registry
        .upsert(&record("lost", Some("2026-11-18T09:30:00.000Z")))
        .unwrap();
    user_message(&timeline, "lost", "2026-11-18T10:02:00.123456+00:00");

    let recovered = recover_status(&registry, &timeline, SINCE).unwrap();

    let restored = registry.get("lost").unwrap().unwrap();
    assert_eq!(recovered, 1);
    let expected = Some("2026-11-18T10:02:00.123Z");
    assert_eq!(restored.last_user_message_at.as_deref(), expected);
    assert_eq!(restored.updated_at, "2026-11-18T10:02:00.123Z");
    assert_eq!(
        restored.last_activity_at.as_deref(),
        Some("2026-11-18T10:03:00.000Z"),
        "a later activity time is kept"
    );
}

#[test]
fn leaves_agents_without_newer_input_unchanged() {
    let (_root, registry, timeline) = fixture();
    let current = record("current", Some("2026-11-18T10:05:00.000Z"));
    let quiet = record("quiet", None);
    registry.upsert(&current).unwrap();
    registry.upsert(&quiet).unwrap();
    user_message(&timeline, "current", "2026-11-18T10:04:00.000Z");
    user_message(&timeline, "quiet", "2026-11-18T09:45:00.000Z");

    let recovered = recover_status(&registry, &timeline, SINCE).unwrap();

    assert_eq!(recovered, 0);
    assert_eq!(registry.get("current").unwrap(), Some(current));
    assert_eq!(registry.get("quiet").unwrap(), Some(quiet));
}

#[test]
fn rejects_an_unreadable_change_time() {
    let (_root, registry, timeline) = fixture();

    assert_eq!(
        recover_status(&registry, &timeline, "yesterday"),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn restores_input_recorded_shortly_before_the_last_write() {
    let (_root, registry, timeline) = fixture();
    registry
        .upsert(&record("steered", Some("2026-11-18T09:30:00.000Z")))
        .unwrap();
    registry
        .upsert(&record("stale", Some("2026-11-18T09:30:00.000Z")))
        .unwrap();
    user_message(&timeline, "steered", "2026-11-18T09:59:58.000Z");
    user_message(&timeline, "stale", "2026-11-18T09:40:00.000Z");

    let recovered = recover_status(&registry, &timeline, SINCE).unwrap();

    assert_eq!(recovered, 1);
    let steered = registry.get("steered").unwrap().unwrap();
    assert_eq!(
        steered.last_user_message_at.as_deref(),
        Some("2026-11-18T09:59:58.000Z")
    );
    let stale = registry.get("stale").unwrap().unwrap();
    assert_eq!(
        stale.last_user_message_at.as_deref(),
        Some("2026-11-18T09:30:00.000Z"),
        "messages older than the margin are not read"
    );
}
