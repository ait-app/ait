use super::*;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use provider_core::ports::preset::Preset;
use std::collections::BTreeMap;
use std::sync::Arc;

#[tokio::test]
async fn preset_creation_waits_until_initial_prompt_is_accepted_and_can_exit() {
    let fixture = Fixture::new();
    let store = Arc::new(FileDaemonConfigStore::with_defaults(
        fixture.root.path().join("config.json"),
    ));
    let presets = crate::service::presets::PresetService::new(store);
    let preset = Preset {
        name: "first".into(),
        model: "changed-model".into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    presets
        .save("codex", fixture.cwd.to_str().unwrap(), None, &preset, None)
        .await
        .unwrap();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(presets),
    );
    let created = tokio::time::timeout(Duration::from_secs(10), execution.execute("agent.create.request", json!({
        "config":{"provider":"codex","cwd":fixture.cwd},
        "modelPreset":{"name":"first","source":"ait","baseFeatureValues":{},"baseThinkingOptionId":null},
        "initialPrompt":"hello"
    }))).await.unwrap().unwrap();
    assert_eq!(created["creation"]["phase"], "completed");
    assert!(
        fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    let id = created["agentId"].as_str().unwrap();
    let context = execution
        .execute("agent.model_preset.context.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(context["selectedModelPreset"]["name"], "first");
    assert!(created["agent"].get("selectedModelPreset").is_none());
    assert_eq!(created["agent"]["hasSelectedModelPreset"], true);
    let changed = execution
        .execute(
            "agent.model.set.request",
            json!({"agentId":id,"modelId":"plain-model"}),
        )
        .await
        .unwrap();
    assert_eq!(changed["accepted"], true);
    let context = execution
        .execute("agent.model_preset.context.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(context["selectedModelPreset"].is_null());
    let snapshot = execution
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(snapshot["agent"]["hasSelectedModelPreset"], false);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn edited_selected_preset_changes_only_next_turn_and_same_model_exits() {
    let fixture = Fixture::new();
    let store = Arc::new(FileDaemonConfigStore::with_defaults(
        fixture.root.path().join("config.json"),
    ));
    let presets = crate::service::presets::PresetService::new(store);
    let make_preset = |model: &str| Preset {
        name: "team".into(),
        model: model.into(),
        thinking_option_id: Some("high".into()),
        feature_values: BTreeMap::new(),
    };
    let cwd = fixture.cwd.to_str().unwrap();
    presets
        .save("codex", cwd, None, &make_preset("first-model"), None)
        .await
        .unwrap();
    let service = presets.clone();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(presets),
    );
    let created = execution.execute("agent.create.request", json!({
        "config":{"provider":"codex","cwd":fixture.cwd,"thinkingOptionId":"low"},
        "modelPreset":{"name":"team","source":"ait","baseFeatureValues":{},"baseThinkingOptionId":"low"},
        "initialPrompt":"hang"
    })).await.unwrap();
    let id = created["agentId"].as_str().unwrap();
    let starts = || {
        fixture
            .requests()
            .into_iter()
            .filter(|request| request["method"] == "turn/start")
            .collect::<Vec<_>>()
    };
    assert_eq!(starts().len(), 1);
    assert_eq!(starts()[0]["params"]["model"], "first-model");
    assert_eq!(starts()[0]["params"]["effort"], "high");
    service
        .save("codex", cwd, None, &make_preset("updated-model"), None)
        .await
        .unwrap();
    assert_eq!(
        starts().len(),
        1,
        "editing must not submit or change the running turn"
    );
    execution
        .execute("agent.cancel.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"second"}),
        )
        .await
        .unwrap();
    execution
        .execute("agent.finish.wait.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(starts()[1]["params"]["model"], "updated-model");
    let result = execution
        .execute(
            "agent.model.set.request",
            json!({"agentId":id,"modelId":"updated-model"}),
        )
        .await
        .unwrap();
    assert_eq!(result["accepted"], true);
    let context = execution
        .execute("agent.model_preset.context.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert!(context["selectedModelPreset"].is_null());
    let snapshot = execution
        .execute("agent.get.request", json!({"agentId":id}))
        .await
        .unwrap();
    assert_eq!(snapshot["agent"]["thinkingOptionId"], "low");
    assert_eq!(snapshot["agent"]["hasSelectedModelPreset"], false);
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn full_queue_marks_initial_creation_failed_without_sending_prompt() {
    use provider_core::protocol::prompt::AgentPrompt;
    let fixture = Fixture::new();
    let creations = model::creation::Creations::default();
    let (execution, registry) = worker_with_creations(&fixture, creations.clone());
    let existing = create(&execution, &fixture).await;
    let mut record = registry
        .get(existing["agentId"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    record.id.clone_from(&id);
    registry.upsert(&record).unwrap();
    let admission = creations
        .begin(
            domain::creation::protocol::Kind::Agent,
            "queue-full",
            json!({"agentId":id}),
        )
        .unwrap();
    let mut state = execution.0.template.lock().unwrap().fork(None);
    let timeline = state.manager.timeline().unwrap();
    for index in 0..32 {
        timeline
            .reserve_input(
                &id,
                &format!("existing-{index}"),
                &AgentPrompt::text("queued"),
                "interrupt",
            )
            .unwrap();
    }
    let result = state
        .finish_creation_checked(
            &id,
            &admission.snapshot,
            Some(AgentPrompt::text("initial")),
            true,
        )
        .await;
    assert!(result.is_err());
    let snapshot = creations
        .snapshot(domain::creation::protocol::Kind::Agent, "queue-full")
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.phase, domain::creation::protocol::Phase::Failed);
    assert!(snapshot.agent.is_some());
    assert!(snapshot.outcome_unknown);
    let initial = id.clone();
    assert!(
        timeline
            .input_delivery_state(&id, &initial)
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| request["method"] != "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn composite_workspace_creation_applies_selected_preset() {
    let fixture = Fixture::new();
    let creations = model::creation::Creations::default();
    let service = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(fixture.root.path().join("config.json")),
    ));
    let preset = Preset {
        name: "team".into(),
        model: "changed-model".into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    service
        .save("codex", fixture.cwd.to_str().unwrap(), None, &preset, None)
        .await
        .unwrap();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        creations.clone(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(service),
    );
    let agent = json!({"config":{"provider":"codex","cwd":fixture.cwd},"modelPreset":{"name":"team","source":"ait"},"initialPrompt":"hello"});
    let admission = creations
        .begin(
            domain::creation::protocol::Kind::Workspace,
            "composite-ok",
            json!({"workspaceId":"wks_0123456789abcdef","agent":agent}),
        )
        .unwrap();
    let ready = creations
        .advance(
            &admission.snapshot,
            domain::creation::protocol::Phase::WorkspaceReady,
            Some(json!({"workspace":{"workspaceId":"wks_0123456789abcdef"}})),
            None,
        )
        .unwrap();
    let created = execution.execute("internal.workspace.agent.create", json!({"agent":agent,"creation":ready,"source":{"kind":"directory","path":fixture.cwd}})).await.unwrap();
    assert_eq!(created["creation"]["phase"], "completed");
    assert_eq!(created["agent"]["hasSelectedModelPreset"], true);
    assert!(
        fixture
            .requests()
            .iter()
            .any(|request| request["method"] == "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_composite_preset_keeps_workspace_and_other_agents() {
    let fixture = Fixture::new();
    let creations = model::creation::Creations::default();
    let service = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(fixture.root.path().join("config.json")),
    ));
    let fence = service.fence().write_owned().await;
    let (execution, registry) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        creations.clone(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(service.clone()),
    );
    let agent = json!({"config":{"provider":"codex","cwd":fixture.cwd},"modelPreset":{"name":"missing","source":"ait"}});
    let admission = creations
        .begin(
            domain::creation::protocol::Kind::Workspace,
            "composite-fail",
            json!({"workspaceId":"wks_0123456789abcdef","agent":agent}),
        )
        .unwrap();
    let ready = creations
        .advance(
            &admission.snapshot,
            domain::creation::protocol::Phase::WorkspaceReady,
            Some(json!({"workspace":{"workspaceId":"wks_0123456789abcdef"}})),
            None,
        )
        .unwrap();
    let pending = {
        let execution = execution.clone();
        let cwd = fixture.cwd.clone();
        tokio::spawn(async move {
            execution.execute("internal.workspace.agent.create", json!({"agent":agent,"creation":ready,"source":{"kind":"directory","path":cwd}})).await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), service.wait_lookup_started())
        .await
        .unwrap();
    assert!(!pending.is_finished(), "preset lookup is still blocked");
    let existing = create(&execution, &fixture).await;
    let existing_id = existing["agentId"].as_str().unwrap();
    drop(fence);
    assert!(pending.await.unwrap().is_err());
    let outcome = creations
        .snapshot(
            domain::creation::protocol::Kind::Workspace,
            "composite-fail",
        )
        .unwrap()
        .unwrap();
    assert_eq!(outcome.phase, domain::creation::protocol::Phase::Failed);
    assert!(outcome.agent.is_none());
    assert!(outcome.error.is_some());
    assert!(registry.get(existing_id).unwrap().is_some());
    {
        let state = execution.0.template.lock().unwrap();
        assert!(
            !state
                .workspaces
                .get("wks_0123456789abcdef")
                .unwrap()
                .unwrap()
                .is_archived()
        );
    }
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn slow_preset_lookup_does_not_block_permission_resolution() {
    let fixture = Fixture::new();
    let service = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(fixture.root.path().join("config.json")),
    ));
    let preset = Preset {
        name: "team".into(),
        model: "model".into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    service
        .save("codex", fixture.cwd.to_str().unwrap(), None, &preset, None)
        .await
        .unwrap();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(service.clone()),
    );
    let created = create(&execution, &fixture).await;
    let id = created["agentId"].as_str().unwrap().to_owned();
    execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"permit-command"}),
        )
        .await
        .unwrap();
    let permission = controls::pending(&execution, &id).await;
    let fence = service.fence().write_owned().await;
    let selection = {
        let execution = execution.clone();
        let id = id.clone();
        tokio::spawn(async move {
            execution
                .execute(
                    "agent.model_preset.select.request",
                    json!({"agentId":id,"name":"team","source":"ait"}),
                )
                .await
        })
    };
    let context = {
        let execution = execution.clone();
        let id = id.clone();
        tokio::spawn(async move {
            execution
                .execute("agent.model_preset.context.request", json!({"agentId":id}))
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        service.wait_lookup_started().await;
        service.wait_lookup_started().await;
    })
    .await
    .unwrap();
    assert!(!selection.is_finished());
    assert!(!context.is_finished());
    tokio::time::timeout(
        Duration::from_secs(2),
        execution.execute(
            "agent.permission.resolve.request",
            json!({"agentId":id,"requestId":permission["id"],"response":{"behavior":"allow"}}),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    drop(fence);
    assert!(selection.await.unwrap().is_ok());
    let context_result = context.await.unwrap();
    assert!(context_result.is_ok() || matches!(context_result, Err(ErrorCode::CatalogBusy)));
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelling_voice_during_preset_refresh_does_not_send_or_block_later_save() {
    let fixture = Fixture::new();
    let service = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(fixture.root.path().join("config.json")),
    ));
    let preset = Preset {
        name: "team".into(),
        model: "model".into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    let cwd = fixture.cwd.to_str().unwrap();
    service
        .save("codex", cwd, None, &preset, None)
        .await
        .unwrap();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(service.clone()),
    );
    let created = execution.execute("agent.create.request", json!({"config":{"provider":"codex","cwd":fixture.cwd},"modelPreset":{"name":"team","source":"ait"}})).await.unwrap();
    let id = created["agentId"].as_str().unwrap().to_owned();
    service.wait_lookup_started().await;
    let fence = service.fence().write_owned().await;
    let cancel = CancellationToken::new();
    let voice = {
        let execution = execution.clone();
        let id = id.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { execution.send_voice(&id, "hello", cancel).await })
    };
    tokio::time::timeout(Duration::from_secs(5), service.wait_lookup_started())
        .await
        .unwrap();
    assert!(!voice.is_finished());
    cancel.cancel();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), voice)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    drop(fence);
    tokio::time::timeout(
        Duration::from_secs(2),
        service.save("codex", cwd, None, &preset, None),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        fixture
            .requests()
            .iter()
            .all(|request| request["method"] != "turn/start")
    );
    execution.shutdown().await.unwrap();
}

#[tokio::test]
async fn retrying_initial_client_message_does_not_start_or_interrupt_a_second_turn() {
    let fixture = Fixture::new();
    let presets = crate::service::presets::PresetService::new(Arc::new(
        FileDaemonConfigStore::with_defaults(fixture.root.path().join("config.json")),
    ));
    let preset = Preset {
        name: "team".into(),
        model: "model".into(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    };
    presets
        .save("codex", fixture.cwd.to_str().unwrap(), None, &preset, None)
        .await
        .unwrap();
    let (execution, _) = crate::testing::spawn(
        fixture.root.path(),
        &fixture.cwd,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
        |manager| manager.with_presets(presets),
    );
    let created = execution.execute("agent.create.request", json!({"config":{"provider":"codex","cwd":fixture.cwd},"modelPreset":{"name":"team","source":"ait"},"initialPrompt":"hang","clientMessageId":"client-1"})).await.unwrap();
    let id = created["agentId"].as_str().unwrap();
    let retried = execution
        .execute(
            "agent.message.send.request",
            json!({"agentId":id,"text":"hang","messageId":"client-1"}),
        )
        .await
        .unwrap();
    assert_eq!(retried["accepted"], true);
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
    assert!(
        requests
            .iter()
            .all(|request| request["method"] != "turn/interrupt")
    );
    execution.shutdown().await.unwrap();
}
