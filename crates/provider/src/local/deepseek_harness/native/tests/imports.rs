use super::fixture::Fixture;
use crate::ports::{
    agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec},
    native_history::ListOptions,
};
use domain::agent_runtime::AgentPersistenceHandle;
use serde_json::{Value, json};

fn handle() -> AgentPersistenceHandle {
    AgentPersistenceHandle {
        provider: "deepseek-harness".into(),
        session_id: "session".into(),
        native_handle: None,
        metadata: None,
    }
}

fn records() -> Vec<Value> {
    [
        ("user/message", json!({"id":"user","source":{"kind":"user"},"content":[{"type":"text","text":"External question"}]})),
        ("turn/start", json!({"turn":1})),
        ("assistant/message", json!({"turn":1,"message":{"id":"answer","content":[{"type":"text","text":"External answer"}]}})),
        ("turn/end", json!({"turn":1,"reason":{"kind":"completed"}})),
    ].into_iter().enumerate().map(|(seq, (kind, data))| json!({"type":"event","event":{"seq":seq,"type":kind,"time":1_700_000_000_000_u64+seq as u64,"data":data}})).collect()
}

#[tokio::test]
async fn external_import_reads_full_history_without_writes_then_restores_the_native_identity() {
    let fixture = Fixture::new().await;
    fixture.seed_records(records());
    fixture.set_sessions(json!([{"sessionId":"session","cwd":fixture.spec.cwd,"blank":false,"updatedAt":1_700_000_000_003_i64}]));
    assert!(fixture.client.supports_session_import());
    assert_eq!(
        fixture.client.settings(&fixture.spec.config)["capabilities"]["supportsSessionListing"],
        true
    );
    let entries = fixture
        .client
        .list_sessions(&ListOptions {
            cwd: Some(fixture.spec.cwd.clone()),
            scan_limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(entries[0].provider_handle_id, "session");
    assert_eq!(
        entries[0].first_prompt_preview.as_deref(),
        Some("External question")
    );
    assert_eq!(
        entries[0].last_prompt_preview.as_deref(),
        Some("External question")
    );
    assert_eq!(
        fixture.requests("session/list"),
        vec![json!({"_request":{}})]
    );
    let mut handle = handle();
    let imported = fixture
        .client
        .inspect_session(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert!(!imported.active);
    assert_eq!(imported.entries.len(), 2);
    assert_eq!(
        imported.descriptor.first_prompt_preview.as_deref(),
        Some("External question")
    );
    assert_eq!(
        imported.config.model.as_deref(),
        Some("[\"local\",\"test\"]")
    );
    assert_eq!(imported.config.mode_id.as_deref(), Some("workspace-write"));
    assert!(!fixture.requests("session/page").is_empty());
    for method in [
        "session/create",
        "session/prompt",
        "session/selectModel",
        "commands/execute",
    ] {
        assert!(
            fixture.requests(method).is_empty(),
            "{method} must not run during import"
        );
    }
    handle.metadata = Some(imported.resume_metadata);
    let serialized = serde_json::to_vec(&handle).unwrap();
    let restored: AgentPersistenceHandle = serde_json::from_slice(&serialized).unwrap();
    let replay = fixture
        .client
        .history(&restored, &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(imported.entries).unwrap()
    );
    let spec = AgentSessionSpec {
        config: imported.config,
        ..fixture.spec.clone()
    };
    let mut resumed = fixture
        .client
        .resume_session(&restored, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, "session");
    resumed.start_turn("continue", &spec.config).await.unwrap();
    assert_eq!(fixture.requests("session/prompt").len(), 1);
    resumed.close().await.unwrap();
}

#[tokio::test]
async fn import_preserves_explicit_model_effort_and_custom_native_permissions() {
    let fixture = Fixture::new().await;
    fixture.seed_records(records());
    fixture.set_snapshot_fields(json!({"projections":{"values":{
        "title":"Imported title", "permissions":{"currentValue":"custom"},
        "modelSelection":{"next":{"provider":"local","model":"test","reasoningEffort":"high"}}
    }}}));
    let imported = fixture
        .client
        .inspect_session(&handle(), &fixture.spec.cwd)
        .await
        .unwrap();
    assert_eq!(imported.config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(imported.config.mode_id, None);
    assert_eq!(imported.descriptor.title.as_deref(), Some("Imported title"));
    assert!(fixture.requests("session/modelCatalog").is_empty());
}

#[tokio::test]
async fn import_detects_active_turns_and_rejects_mismatched_or_incomplete_history() {
    let fixture = Fixture::new().await;
    let mut active = records();
    active.pop();
    fixture.seed_records(active);
    assert!(
        fixture
            .client
            .inspect_session(&handle(), &fixture.spec.cwd)
            .await
            .unwrap()
            .active
    );
    fixture.seed_records(vec![records().remove(2)]);
    assert!(
        fixture
            .client
            .inspect_session(&handle(), &fixture.spec.cwd)
            .await
            .is_err()
    );
    fixture.seed_records(records());
    for header in [
        json!({"id":"different","cwd":fixture.spec.cwd}),
        json!({"id":"session","cwd":"/elsewhere"}),
        json!({"id":"session","cwd":fixture.spec.cwd,"origin":"subagent"}),
    ] {
        fixture.set_snapshot_fields(json!({"header":header}));
        assert!(
            fixture
                .client
                .inspect_session(&handle(), &fixture.spec.cwd)
                .await
                .is_err()
        );
    }
    assert!(fixture.requests("session/create").is_empty());
}

#[tokio::test]
async fn acp_does_not_advertise_or_attempt_native_import_and_invalid_requests_do_not_launch() {
    let fixture = Fixture::new().await;
    let acp = fixture.client.clone().with_acp_profile();
    assert!(!acp.supports_session_import());
    assert!(
        acp.list_sessions(&ListOptions {
            cwd: None,
            scan_limit: 20
        })
        .await
        .is_err()
    );
    assert!(
        acp.inspect_session(&handle(), &fixture.spec.cwd)
            .await
            .is_err()
    );
    for scan_limit in [0, 4097] {
        assert!(
            fixture
                .client
                .list_sessions(&ListOptions {
                    cwd: None,
                    scan_limit
                })
                .await
                .is_err()
        );
    }
    let mut invalid = handle();
    invalid.provider = "opencode".into();
    assert!(
        fixture
            .client
            .inspect_session(&invalid, &fixture.spec.cwd)
            .await
            .is_err()
    );
    assert!(fixture.requests("session/list").is_empty());
}

fn manager(fixture: &Fixture) -> crate::service::agent_manager::AgentManager {
    let root = std::path::Path::new(&fixture.spec.cwd);
    let registry = crate::storage::agent_runtime::FileBackedAgentRuntimeRegistry::new(
        root.join("agents.json"),
    );
    let timeline = crate::storage::timeline::Timeline::open(&root.join("timeline.sqlite")).unwrap();
    let mut manager = crate::service::agent_manager::AgentManager::new(Box::new(registry))
        .with_timeline(timeline);
    manager
        .register_client(Box::new(fixture.client.clone()))
        .unwrap();
    manager
}

#[tokio::test]
async fn application_import_persists_metadata_deduplicates_and_reopens_for_continuation() {
    let fixture = Fixture::new().await;
    fixture.seed_records(records());
    fixture.set_sessions(
        json!([{"sessionId":"session","cwd":fixture.spec.cwd,"updatedAt":1_700_000_000_003_i64}]),
    );
    let mut application = manager(&fixture);
    let request = || {
        serde_json::from_value(json!({"providers":["deepseek-harness"],"cwd":fixture.spec.cwd}))
            .unwrap()
    };
    assert_eq!(
        application.recent_sessions(request()).await.unwrap()["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let imported = application
        .inspect_native(&handle(), &fixture.spec.cwd)
        .await
        .unwrap();
    let record = serde_json::from_value(json!({
        "id":"imported-agent", "provider":"deepseek-harness", "cwd":fixture.spec.cwd,
        "createdAt":imported.created_at,"updatedAt":imported.descriptor.last_activity_at,
        "config":imported.config,"persistence":handle()
    }))
    .unwrap();
    application.import_native(record, &imported, false).unwrap();
    assert_eq!(
        application.recent_sessions(request()).await.unwrap()["entries"],
        json!([])
    );
    assert!(fixture.requests("session/create").is_empty());
    drop(application);
    let mut reopened = manager(&fixture);
    reopened.load_timeline("imported-agent").await.unwrap();
    assert_eq!(
        reopened
            .timeline()
            .unwrap()
            .read("imported-agent")
            .unwrap()
            .1
            .len(),
        2
    );
    let record = reopened.resume("imported-agent").await.unwrap();
    assert_eq!(record.persistence.unwrap().session_id, "session");
    reopened
        .send("imported-agent", "continue after restart")
        .await
        .unwrap();
    assert_eq!(fixture.requests("session/prompt").len(), 1);
    reopened.close_all().await.unwrap();
}

#[tokio::test]
async fn application_rejects_importing_an_unfinished_native_turn() {
    let fixture = Fixture::new().await;
    let mut active = records();
    active.pop();
    fixture.seed_records(active);
    let application = manager(&fixture);
    assert_eq!(
        application
            .inspect_native(&handle(), &fixture.spec.cwd)
            .await
            .unwrap_err(),
        model::ErrorCode::CatalogBusy
    );
    assert!(fixture.requests("session/create").is_empty());
}

#[tokio::test]
async fn missing_preview_history_does_not_hide_discovered_sessions() {
    let fixture = Fixture::new().await;
    fixture.set_sessions(json!([
        {"sessionId":"missing","cwd":fixture.spec.cwd,"updatedAt":2},
        {"sessionId":"session","cwd":fixture.spec.cwd,"updatedAt":1,"projections":{"values":{"turnOutline":[{"prompt":"cached question"}]}}}
    ]));
    let entries = fixture
        .client
        .list_sessions(&ListOptions {
            cwd: None,
            scan_limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].first_prompt_preview.is_none());
    assert_eq!(
        entries[1].first_prompt_preview.as_deref(),
        Some("cached question")
    );
    assert!(fixture.requests("session/create").is_empty());
}
