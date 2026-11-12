use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::json;

use super::*;
use crate::local::codex::CodexClient;

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

    let page = fetch_timeline(
        &timeline,
        "agent",
        json!({"agentId":"agent","direction":"tail","limit":30}),
        &Value::Null,
        Some(false),
    )
    .unwrap();
    assert_eq!(page["entries"][0]["item"]["text"], "second");
    assert_eq!(
        fetch_timeline(
            &timeline,
            "agent",
            json!({"limit":"all"}),
            &Value::Null,
            None
        ),
        Err(ErrorCode::InvalidMessage)
    );

    progress_timeline(
        &timeline,
        "agent",
        "omp",
        "observation",
        &entry("native:c", "third"),
    )
    .unwrap();
    let (_, entries) = timeline_entries(&timeline, "agent").unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].item["text"], "third");
}

#[tokio::test]
async fn manager_delegates_registration_and_reports_unknown_agents() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));
    // A missing absolute program is never resolved on PATH, so no installed Codex starts.
    let program = root.path().join("missing-codex");

    manager
        .register(Box::new(CodexClient::new(program.clone())))
        .unwrap();
    assert!(matches!(
        manager.register(Box::new(CodexClient::new(program))),
        Err(AgentManagerError::AlreadyExists(provider)) if provider == "codex"
    ));
    manager.validate_provider("codex").unwrap();
    assert!(matches!(
        manager.validate_provider("missing"),
        Err(AgentManagerError::ProviderUnavailable(provider)) if provider == "missing"
    ));
    assert_eq!(manager.active_turn("missing"), None);
    assert_eq!(manager.last_message("missing"), None);
    assert!(manager.live_snapshot("missing").is_none());
    assert!(matches!(
        manager.configure("missing", &ConfigPatch::default()).await,
        Err(AgentManagerError::NotFound(agent)) if agent == "missing"
    ));
    manager.close("missing").await.unwrap();
    assert!(manager.send("missing", "hello").await.is_err());
    assert!(
        manager
            .send_input("missing", &AgentPrompt::text("hello"))
            .await
            .is_err()
    );
    assert!(manager.restore("missing").await.is_err());
    let spec = AgentSessionSpec {
        provider: "missing".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
        confinement: None,
    };
    assert!(
        manager
            .create_with_environment("agent", &spec, &AgentEnvironment::default())
            .await
            .is_err()
    );
    assert!(matches!(
        manager.create_internal("agent", &spec, json!({"botId":"bot"})).await,
        Err(AgentManagerError::ProviderUnavailable(provider)) if provider == "missing"
    ));
    // This manager has no display timeline, so there is nowhere to load history into.
    assert_eq!(
        manager.load_timeline("missing").await,
        Err(ErrorCode::UnsupportedCapability)
    );
    assert_eq!(
        manager.usage(None, Some("missing"), false).await,
        Err(ErrorCode::UnsupportedCapability)
    );
    let snapshot = manager
        .providers("provider.snapshot.get.request", json!({"cwd":root.path()}))
        .await
        .unwrap();
    assert_eq!(snapshot["entries"][0]["provider"], "codex");
    assert_eq!(snapshot["entries"][0]["status"], "unavailable");
    let _events = manager.events().connect();
    manager.poll().await.unwrap();
    manager.close_all().await.unwrap();
}
