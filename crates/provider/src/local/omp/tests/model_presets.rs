use super::*;

fn save(root: &std::path::Path, scope: &str, value: &Value) {
    std::fs::create_dir_all(root.join("agent")).unwrap();
    std::fs::write(
        root.join(format!("agent/{scope}-presets.json")),
        value.to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn presets_use_whole_project_entries_and_reuse_resolved_models_on_refresh() {
    let (root, client, mut spec) = fixture();
    save(
        root.path(),
        "global",
        &json!({"work":{"modelRoles":{"default":"test/model","advisor":"test/model"},"defaultThinkingLevel":"low"}}),
    );
    save(
        root.path(),
        "project",
        &json!({"work":{"modelRoles":{"default":"@smol","smol":"other/model:high"},"defaultThinkingLevel":"high"}}),
    );
    let features = client.discover(&spec.cwd).await.unwrap().features;
    let presets = features
        .iter()
        .find(|feature| feature["id"] == "modelPreset")
        .unwrap();
    assert_eq!(presets["options"][1]["metadata"]["source"], "project");
    spec.config.feature_values = Some(BTreeMap::from([("modelPreset".to_owned(), json!("work"))]));
    let resolved = client.resolve_selection(&spec).await.unwrap();
    assert_eq!(resolved.model, Some(json!(["other", "model"]).to_string()));
    assert_eq!(resolved.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        resolved.feature_values.as_ref().unwrap()["modelRole.advisor"],
        "unset"
    );
    let log = root.path().join("agent/calls.jsonl");
    let readers = || {
        std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .filter(|call| call.get("args").is_some())
            .count()
    };
    let before = readers();
    client.discover(&spec.cwd).await.unwrap();
    assert_eq!(
        readers(),
        before + 1,
        "only the shared catalog reader should start again"
    );
    assert_eq!(std::fs::read_to_string(root.path().join("agent/global-presets.json")).unwrap(),json!({"work":{"modelRoles":{"default":"test/model","advisor":"test/model"},"defaultThinkingLevel":"low"}}).to_string());
}

#[tokio::test]
async fn existing_agents_apply_presets_atomically_then_allow_individual_edits_and_restart() {
    use crate::protocol::agent_config::ConfigPatch;
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::registry::AgentRuntimeRegistry;
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    save(
        root.path(),
        "global",
        &json!({"deep":{"modelRoles":{"default":"other/model","advisor":"test/model:high","custom":"other/model"},"defaultThinkingLevel":"high"}}),
    );
    client.discover(&spec.cwd).await.unwrap();
    let patch: ConfigPatch =
        serde_json::from_value(json!({"featureValues":{"modelPreset":"deep","autoRetry":false}}))
            .unwrap();
    manager.configure("agent", &patch).await.unwrap();
    let config = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(config.model, Some(json!(["other", "model"]).to_string()));
    assert_eq!(config.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        config.feature_values.as_ref().unwrap()["modelRole.custom"],
        "other/model"
    );
    assert_eq!(config.feature_values.as_ref().unwrap()["autoRetry"], false);
    manager
        .configure(
            "agent",
            &serde_json::from_value(
                json!({"featureValues":{"modelRole.advisor":"other/model:low"}}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    save(root.path(), "global", &json!({}));
    client.discover(&spec.cwd).await.unwrap();
    manager.send("agent", "hello").await.unwrap();
    while manager.active_turn("agent").is_some() {
        manager.poll().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    manager.close_all().await.unwrap();
    manager.resume("agent").await.unwrap();
    let stored = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(
        stored.feature_values.as_ref().unwrap()["modelRole.advisor"],
        "other/model:low"
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn malformed_presets_remain_visible_and_reject_without_partial_writes() {
    let (root, client, mut spec) = fixture();
    save(
        root.path(),
        "global",
        &json!({"broken":{"modelRoles":false},"valid":{"modelRoles":{"default":"test/model"}}}),
    );
    let features = client.discover(&spec.cwd).await.unwrap().features;
    let preset = features
        .iter()
        .find(|feature| feature["id"] == "modelPreset")
        .unwrap();
    assert!(preset["options"][1]["metadata"]["error"].is_string());
    spec.config.feature_values = Some(BTreeMap::from([(
        "modelPreset".to_owned(),
        json!("broken"),
    )]));
    assert_eq!(
        client.resolve_selection(&spec).await.unwrap_err(),
        AgentSessionError::Rejected
    );
    spec.config
        .feature_values
        .as_mut()
        .unwrap()
        .insert("modelPreset".to_owned(), json!("missing"));
    assert_eq!(
        client.resolve_selection(&spec).await.unwrap_err(),
        AgentSessionError::Rejected
    );
}

#[tokio::test]
async fn native_fallback_and_missing_resolver_are_not_reported_as_preset_success() {
    for (name, roles, missing) in [
        ("gone", json!({"default":"missing/model"}), false),
        ("helper", json!({"default":"test/model"}), true),
    ] {
        let (root, client, mut spec) = fixture();
        save(root.path(), "global", &json!({name:{"modelRoles":roles}}));
        if missing {
            std::fs::write(root.path().join("agent/missing-preset-helper"), "").unwrap();
        }
        client.discover(&spec.cwd).await.unwrap();
        spec.config.feature_values =
            Some(BTreeMap::from([("modelPreset".to_owned(), json!(name))]));
        assert_eq!(
            client.resolve_selection(&spec).await.unwrap_err(),
            AgentSessionError::Rejected
        );
        if missing {
            let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
            assert!(
                !calls
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .any(|call| call["type"] == "prompt")
            );
        }
    }
}

#[tokio::test]
async fn global_preset_management_saves_latest_next_turn_selection_and_preserves_project_ownership()
{
    let (root, client, mut spec) = fixture();
    let mut owner = client.create_session(&spec).await.unwrap();
    save(
        root.path(),
        "global",
        &json!({"team":{"modelRoles":{"default":"test/model"},"defaultThinkingLevel":"low"}}),
    );
    std::fs::write(root.path().join("agent/preset-write-delay"), "").unwrap();
    spec.config.model = Some(json!(["other", "model"]).to_string());
    spec.config.thinking_option_id = Some("high".to_owned());
    let saved = owner
        .begin_configured_action("model_preset_save", &json!({"name":"team"}), &spec.config)
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(saved["error"].is_null(), "{saved}");
    let global: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("agent/global-presets.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(global["team"]["modelRoles"]["default"], "other/model:high");
    assert_eq!(global["team"]["defaultThinkingLevel"], "high");
    save(
        root.path(),
        "project",
        &json!({"team":{"modelRoles":{"default":"test/model"}}}),
    );
    let deleted = owner
        .begin_configured_action("model_preset_delete", &json!({"name":"team"}), &spec.config)
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(
        deleted["error"].is_string(),
        "project preset remains; do not claim complete deletion"
    );
    let global: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("agent/global-presets.json")).unwrap(),
    )
    .unwrap();
    assert!(global.get("team").is_none());
    assert!(
        std::fs::read_to_string(root.path().join("agent/project-presets.json"))
            .unwrap()
            .contains("team")
    );
    assert!(
        owner
            .begin_configured_action(
                "model_preset_save",
                &json!({"name":"../invalid"}),
                &spec.config
            )
            .await
            .is_err()
    );
    let missing = owner
        .begin_configured_action(
            "model_preset_delete",
            &json!({"name":"missing"}),
            &spec.config,
        )
        .await
        .unwrap()
        .await
        .unwrap();
    assert!(missing["error"].is_string());
    std::fs::write(root.path().join("agent/missing-preset-builtin"), "").unwrap();
    let unsupported = owner
        .begin_configured_action(
            "model_preset_save",
            &json!({"name":"unsupported"}),
            &spec.config,
        )
        .await
        .unwrap()
        .await;
    assert_eq!(unsupported.unwrap_err(), AgentSessionError::Unavailable);
    let calls = std::fs::read_to_string(root.path().join("agent/calls.jsonl")).unwrap();
    assert!(
        !calls
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .any(|call| call["message"] == "/modelpreset save unsupported")
    );
    owner.close().await.unwrap();
}
