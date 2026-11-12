use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::json;

use super::*;
use provider_codex::CodexClient;
use provider_core::ports::native_history::CwdLookup;

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

#[test]
fn timeline_rows_keep_sequences_and_projection_merges_one_tool_lifecycle() {
    let tool = |key: &str, status: &str| NativeItem {
        item: json!({"type":"tool_call","callId":"call","name":"shell","status":status}),
        ..entry(key, "")
    };
    let entries = [
        tool("running", "running"),
        tool("done", "completed"),
        entry("reply", "after"),
    ];
    let timeline = memory_timeline().unwrap();
    let (epoch, positions) = append_timeline(&timeline, "agent", "codex", &entries).unwrap();

    let (read, rows) = timeline_rows(&timeline, "agent").unwrap();
    assert_eq!(read, epoch);
    assert_eq!(rows, positions.into_iter().zip(entries).collect::<Vec<_>>());

    let items = projected_timeline_items(&timeline, "agent").unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["callId"], "call");
    assert_eq!(items[0]["status"], "completed");
    assert_eq!(items[1]["text"], "after");
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
    assert_eq!(manager.0.cwd_lookup("missing"), CwdLookup::Canonical);
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

#[tokio::test]
async fn manager_delegates_native_session_listing_inspection_and_import() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let cwd = root.path().to_str().unwrap().to_owned();
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));

    assert!(manager.timeline().is_none());
    assert_eq!(
        manager.recent_sessions(json!({"limit":"many"})).await,
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        manager
            .recent_sessions(json!({"providers":["missing"]}))
            .await,
        Err(ErrorCode::InvalidMessage)
    );
    let handle = AgentPersistenceHandle {
        provider: "missing".to_owned(),
        session_id: "session".to_owned(),
        native_handle: None,
        metadata: None,
    };
    assert_eq!(
        manager.inspect_native(&handle, &cwd).await.unwrap_err(),
        ErrorCode::UnsupportedCapability
    );
    let history = SessionHistory {
        resume_metadata: std::collections::BTreeMap::new(),
        parent_id: None,
        descriptor: provider_core::ports::native_history::SessionDescriptor {
            provider_id: "missing".to_owned(),
            provider_label: "Missing".to_owned(),
            provider_handle_id: "session".to_owned(),
            cwd: cwd.clone(),
            title: None,
            first_prompt_preview: None,
            last_prompt_preview: None,
            last_activity_at: "2026-11-12T00:00:00Z".to_owned(),
        },
        created_at: "2026-11-12T00:00:00Z".to_owned(),
        config: domain::agent_runtime::StoredAgentConfig::default(),
        active: false,
        entries: Vec::new(),
    };
    let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"imported", "provider":"missing", "cwd":cwd,
        "createdAt":"2026-11-12T00:00:00Z", "updatedAt":"2026-11-12T00:00:00Z",
        "persistence":handle
    }))
    .unwrap();
    // Without a display timeline the manager has nowhere to put the imported history.
    assert_eq!(
        manager.import_native(record, &history, false).unwrap_err(),
        ErrorCode::UnsupportedCapability
    );

    let with_timeline = Manager::new(
        AgentManager::new(Box::new(FileBackedAgentRuntimeRegistry::new(
            root.path().join("other.json"),
        )))
        .with_timeline(Timeline::memory().unwrap()),
    );
    assert!(with_timeline.timeline().is_some());
}

#[tokio::test]
async fn manager_delegates_subagent_reads_owner_lanes_and_catalog_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let cwd = root.path().to_str().unwrap().to_owned();
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));
    // A missing absolute program is never resolved on PATH, so no installed Codex starts.
    manager
        .register(Box::new(CodexClient::new(
            root.path().join("missing-codex"),
        )))
        .unwrap();

    assert_eq!(
        manager.subagents("missing").await,
        Err(ErrorCode::AgentNotFound)
    );
    let child = NativeSubagent {
        persistence: None,
        id: "child".to_owned(),
        parent_id: "missing".to_owned(),
        cwd: cwd.clone(),
        descriptor: json!({}),
    };
    assert_eq!(
        manager.child_history("missing", &child).await.unwrap_err(),
        ErrorCode::AgentNotFound
    );

    let lanes = Lanes::default();
    assert_eq!(lanes.agent("unbound").unwrap(), "agent:unbound");
    let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"agent", "provider":"codex", "cwd":cwd,
        "createdAt":"2026-11-12T00:00:00Z", "updatedAt":"2026-11-12T00:00:00Z",
        "persistence":{"provider":"codex","sessionId":"session"}
    }))
    .unwrap();
    lanes.bind("first", &record).unwrap();
    assert_eq!(lanes.agent("agent").unwrap(), "first");
    assert_eq!(lanes.bind("second", &record), Err(ErrorCode::CatalogBusy));
    let forked = manager.fork(&lanes, "first");
    forked.validate_provider("codex").unwrap();

    manager.close_providers().await;
    assert_eq!(
        forked
            .0
            .read_providers("provider.snapshot.get.request", json!({"cwd":cwd}))
            .await,
        Err(ErrorCode::ServerDraining)
    );
}

#[tokio::test]
async fn manager_delegates_unknown_restores_and_command_lists() {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let cwd = root.path().to_str().unwrap().to_owned();
    let mut manager = Manager::new(AgentManager::new(Box::new(registry)));
    // A missing absolute program is never resolved on PATH, so no installed Codex starts.
    manager
        .register(Box::new(CodexClient::new(
            root.path().join("missing-codex"),
        )))
        .unwrap();
    assert!(!manager.has_sessions());

    let record: PersistedAgentRuntimeRecord = serde_json::from_value(json!({
        "id":"imported", "provider":"missing", "cwd":cwd,
        "createdAt":"2026-11-12T00:00:00Z", "updatedAt":"2026-11-12T00:00:00Z",
        "config":{"thinkingOptionId":" high "},
        "persistence":{"provider":"missing","sessionId":"session"}
    }))
    .unwrap();
    assert_eq!(effective_thinking_option_id(&record), Some("high"));
    assert!(matches!(
        manager.restore_new(record).await,
        Err(AgentManagerError::ProviderUnavailable(provider)) if provider == "missing"
    ));
    assert!(!manager.has_sessions());
    let spec = AgentSessionSpec {
        provider: "missing".to_owned(),
        cwd,
        config: domain::agent_runtime::StoredAgentConfig::default(),
        confinement: None,
    };
    assert_eq!(
        manager.commands(&spec).await,
        Err(ErrorCode::UnsupportedCapability)
    );
}

#[tokio::test]
async fn worker_at_registers_the_given_directory_and_reopens_its_root() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("project");
    std::fs::create_dir(&cwd).unwrap();
    // A missing absolute program is never resolved on PATH, so no installed Codex starts.
    let client = || Box::new(CodexClient::new(root.path().join("missing-codex")));

    let (execution, registry) = worker_at(root.path(), &cwd, Creations::default(), client());
    execution.shutdown().await.unwrap();
    let (restarted, _) = worker_at(root.path(), &cwd, Creations::default(), client());
    restarted.shutdown().await.unwrap();

    let workspaces: Value = serde_json::from_str(
        &std::fs::read_to_string(root.path().join("workspaces.json")).unwrap(),
    )
    .unwrap();
    assert!(
        workspaces
            .to_string()
            .contains(&format!("\"cwd\":{}", json!(cwd)))
    );
    assert!(registry.list().unwrap().is_empty());
    assert!(root.path().join("timeline.sqlite").exists());
}
