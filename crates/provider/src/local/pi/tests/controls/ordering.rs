//! Thinking notifications received during state requests supersede older snapshots.

use super::*;

fn state_response(directory: &TempDir, snapshot: &str, native: &str, levels: &Value) {
    std::fs::write(
        directory.path().join("state-notifications.json"),
        serde_json::to_vec(&json!({
            "snapshot":snapshot,"nativeLevel":native,"levels":levels
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn intervening_thinking_notifications_survive_control_persistence_and_resume() {
    let (directory, client, mut spec) = fixture("thinking-readback");
    spec.config.thinking_option_id = Some("medium".into());
    let registry = FileBackedAgentRuntimeRegistry::new(directory.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    manager
        .create_with_environment(
            "agent",
            &spec,
            AgentRegistration::default(),
            &client.environment,
        )
        .await
        .unwrap();
    state_response(&directory, "medium", "ultra", &json!(["low", "ultra"]));
    manager
        .configure(
            "agent",
            &ConfigPatch {
                thinking_option_id: NullableSetting::Set("high".into()),
                ..ConfigPatch::default()
            },
        )
        .await
        .unwrap();
    manager.poll().await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("ultra")
    );
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("ultra")
    );
    let identity = record.persistence.unwrap().session_id;
    manager.close_all().await.unwrap();
    let requests = directory.path().join("requests.jsonl");
    let before = std::fs::read_to_string(&requests).unwrap();
    let mut restored = AgentManager::new(Box::new(registry));
    restored.register_client(Box::new(client)).unwrap();
    let record = restored
        .restore("agent", &crate::protocol::resume::Overrides::default())
        .await
        .unwrap();
    assert_eq!(record.persistence.unwrap().session_id, identity);
    assert_eq!(
        record.runtime_info.unwrap().thinking_option_id.as_deref(),
        Some("ultra")
    );
    let after = std::fs::read_to_string(&requests).unwrap();
    let controls: Vec<Value> = after[before.len()..]
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|command| command["type"] == "set_thinking_level")
        .collect();
    assert_eq!(controls.len(), 1);
    assert_eq!(controls[0]["level"], "ultra");
    restored.close_all().await.unwrap();
}

#[tokio::test]
async fn repeated_and_malformed_notifications_do_not_restore_stale_thinking() {
    let (directory, client, mut spec) = fixture("thinking-readback");
    spec.config.thinking_option_id = Some("high".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session.runtime_info().await.unwrap();
    for levels in [
        json!(["high"]),
        json!([null, 42, "low", "high", "", "bad\nlevel"]),
    ] {
        state_response(&directory, "medium", "high", &levels);
        assert_eq!(
            session
                .runtime_info()
                .await
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("high")
        );
        assert_eq!(
            session
                .effective_config()
                .unwrap()
                .thinking_option_id
                .as_deref(),
            Some("high")
        );
    }
    // The setter's notification precedes this request. It must not override a later
    // snapshot when no valid notification arrives during the state request itself.
    spec.config.thinking_option_id = Some("minimal".into());
    state_response(&directory, "low", "low", &json!([null, 42, "bad\nlevel"]));
    let ConfigApplication::Applied(info) = session
        .apply_config(
            &spec.config,
            &ConfigPatch {
                thinking_option_id: NullableSetting::Set("minimal".into()),
                ..ConfigPatch::default()
            },
        )
        .await
        .unwrap()
    else {
        panic!("idle controls must return native runtime facts");
    };
    assert_eq!(info.thinking_option_id.as_deref(), Some("low"));
    state_response(&directory, "minimal", "minimal", &json!([]));
    assert_eq!(
        session
            .runtime_info()
            .await
            .unwrap()
            .thinking_option_id
            .as_deref(),
        Some("minimal")
    );
    session.close().await.unwrap();
}
