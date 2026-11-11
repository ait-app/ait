use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::json;

use super::*;
use crate::local::pi::PiClient;

fn entry(key: &str, text: &str) -> NativeItem {
    NativeItem {
        key: key.to_owned(),
        turn_id: None,
        timestamp: "2026-11-11T00:00:00Z".to_owned(),
        item: json!({"type":"assistant_message","text":text}),
    }
}

#[test]
fn timeline_access_reads_appended_and_reconciled_entries() {
    let timeline = memory_timeline().unwrap();
    let _events = timeline_events(&timeline);
    let (appended, _) = append_timeline(&timeline, "agent", "omp", &[entry("a", "first")]).unwrap();

    let (epoch, entries) = timeline_entries(&timeline, "agent").unwrap();
    assert_eq!(epoch, appended);
    assert_eq!(entries, [entry("a", "first")]);

    let replaced = reconcile_timeline(&timeline, "agent", "omp", &[entry("b", "second")]).unwrap();
    assert_ne!(replaced, appended);
    assert_eq!(
        timeline_entries(&timeline, "agent").unwrap(),
        (replaced, vec![entry("b", "second")])
    );
    assert!(timeline_subagents(&timeline, "agent").unwrap().is_empty());
}

#[tokio::test]
async fn manager_delegates_registration_and_reports_unknown_agents() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));

    manager
        .register(Box::new(PiClient::new("pi".into())))
        .unwrap();
    assert!(matches!(
        manager.register(Box::new(PiClient::new("pi".into()))),
        Err(AgentManagerError::AlreadyExists(provider)) if provider == "pi"
    ));
    assert_eq!(manager.active_turn("missing"), None);
    assert!(manager.live_snapshot("missing").is_none());
    assert!(manager.send("missing", "hello").await.is_err());
    manager.poll().await.unwrap();
    manager.close_all().await.unwrap();
}
