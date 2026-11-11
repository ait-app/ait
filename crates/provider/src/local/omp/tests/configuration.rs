//! Native-confirmed configuration: field ack, partial success, durable facts and recovery.

use super::*;
use crate::protocol::agent_config::ConfigPatch;
use crate::service::agent_manager::{AgentManager, AgentManagerError, AgentRegistration};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

fn manager_with(
    root: &tempfile::TempDir,
    client: &OmpClient,
) -> (AgentManager, FileBackedAgentRuntimeRegistry) {
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client.clone())).unwrap();
    (manager, registry)
}

#[tokio::test]
async fn rejected_thinking_keeps_model_and_prior_value_and_reports_the_field() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
        "thinkingOptionId": "invented",
    }))
    .unwrap();

    let rejected = manager.configure("agent", &patch).await.unwrap();

    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].field, "thinkingOptionId");
    let record = registry.get("agent").unwrap().unwrap();
    let config = record.config.unwrap();
    assert_eq!(config.model.as_deref(), Some("[\"other\",\"model\"]"));
    assert_eq!(config.thinking_option_id, None);
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn transport_failure_after_model_ack_persists_confirmed_model_despite_error() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    // The fixture acknowledges set_model, then exits before answering the next command.
    std::fs::write(root.path().join("agent").join("fail-after-setter"), "").unwrap();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
    }))
    .unwrap();

    let result = manager.configure("agent", &patch).await;

    assert!(
        result.is_err(),
        "the closed transport must surface an error"
    );
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().model.as_deref(),
        Some("[\"other\",\"model\"]"),
        "the acknowledged model must be durable even though the request errored"
    );
    assert_eq!(
        record.runtime_info.unwrap().model.as_deref(),
        Some("[\"other\",\"model\"]"),
        "the observed runtime fact must be durable too"
    );
    manager.close_all().await.unwrap();

    // Restart holds the confirmed selection: a fresh manager resumes from the record.
    let (mut restarted, registry) = manager_with(&root, &client);
    restarted.resume("agent").await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap().model.as_deref(),
        Some("[\"other\",\"model\"]")
    );
    let info = restarted
        .live_snapshot("agent")
        .unwrap()
        .runtime_info
        .clone()
        .unwrap();
    assert_eq!(info.model.as_deref(), Some("[\"other\",\"model\"]"));
    restarted.close_all().await.unwrap();
}

#[tokio::test]
async fn partial_ack_persists_only_acknowledged_fields_after_transport_error() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    // set_model is acknowledged; the following thinking-levels request gets EOF.
    std::fs::write(root.path().join("agent").join("fail-after-setter"), "").unwrap();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
        "thinkingOptionId": "high",
        "featureValues": {"autoCompaction": true},
    }))
    .unwrap();

    let result = manager.configure("agent", &patch).await;

    assert!(
        result.is_err(),
        "the closed transport must surface an error"
    );
    let record = registry.get("agent").unwrap().unwrap();
    let config = record.config.unwrap();
    assert_eq!(
        config.model.as_deref(),
        Some("[\"other\",\"model\"]"),
        "the acknowledged model must be durable even though the request errored"
    );
    assert_eq!(
        config.thinking_option_id, None,
        "the unacknowledged thinking selection must not be claimed"
    );
    assert!(
        config
            .feature_values
            .as_ref()
            .and_then(|values| values.get("autoCompaction"))
            .is_none(),
        "the unacknowledged feature toggle must not be claimed"
    );
    assert_eq!(
        record.runtime_info.unwrap().model.as_deref(),
        Some("[\"other\",\"model\"]"),
        "the observed runtime fact must reflect the acknowledged model"
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn unreadable_transport_stays_uncertain_without_false_rollback() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let before = registry.get("agent").unwrap().unwrap().config.unwrap();
    // The next setter dies before acknowledging: neither the write nor a readback completes.
    std::fs::write(root.path().join("agent").join("die-on-next-setter"), "").unwrap();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
    }))
    .unwrap();

    let result = manager.configure("agent", &patch).await;

    assert!(result.is_err());
    let record = registry.get("agent").unwrap().unwrap();
    assert_eq!(
        record.config.unwrap(),
        before,
        "an uncertain outcome must not publish a proposed or rolled-back value"
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn plugin_model_change_is_persisted_and_survives_restart() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    // A native plugin changes the model outside any Ait request: the fixture emits an explicit
    // config_update when the prompt text asks for it, without creating a turn.
    manager.send("agent", "plugin-config").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            manager.poll().await.unwrap();
            let record = registry.get("agent").unwrap().unwrap();
            if record.config.as_ref().unwrap().model.as_deref() == Some("[\"other\",\"model\"]") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    // The accepted native change is persisted as the stored selection, not just a runtime fact.
    assert_eq!(
        record.config.unwrap().thinking_option_id.as_deref(),
        Some("high")
    );
    manager.close_all().await.unwrap();

    // Restart preserves the actual selection: resume reopens with the persisted config rather
    // than reverting to the pre-change value.
    let (mut restarted, registry) = manager_with(&root, &client);
    restarted.resume("agent").await.unwrap();
    let record = registry.get("agent").unwrap().unwrap();
    let config = record.config.unwrap();
    assert_eq!(config.model.as_deref(), Some("[\"other\",\"model\"]"));
    assert_eq!(config.thinking_option_id.as_deref(), Some("high"));
    restarted.close_all().await.unwrap();
}

#[tokio::test]
async fn busy_turn_rejects_configuration_before_any_native_mutation() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.send("agent", "wait").await.unwrap();
    let before = registry.get("agent").unwrap().unwrap().config.unwrap();
    let patch: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
    }))
    .unwrap();

    let result = manager.configure("agent", &patch).await;

    assert!(matches!(result, Err(AgentManagerError::Busy)));
    assert_eq!(
        registry.get("agent").unwrap().unwrap().config.unwrap(),
        before
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn clearing_overrides_keeps_absent_clear_set_distinct() {
    let (root, client, spec) = fixture();
    let (mut manager, registry) = manager_with(&root, &client);
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let set: ConfigPatch = serde_json::from_value(json!({
        "modelId": "[\"other\",\"model\"]",
        "thinkingOptionId": "high",
    }))
    .unwrap();
    manager.configure("agent", &set).await.unwrap();
    let config = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(config.model.as_deref(), Some("[\"other\",\"model\"]"));
    assert_eq!(config.thinking_option_id.as_deref(), Some("high"));

    // Omitted fields keep their values.
    let omitted: ConfigPatch = serde_json::from_value(json!({})).unwrap();
    manager.configure("agent", &omitted).await.unwrap();
    let config = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(config.model.as_deref(), Some("[\"other\",\"model\"]"));

    // Explicit null clears the host override; the native selection stays until reopen.
    let clear: ConfigPatch =
        serde_json::from_value(json!({"modelId": null, "thinkingOptionId": null})).unwrap();
    manager.configure("agent", &clear).await.unwrap();
    let config = registry.get("agent").unwrap().unwrap().config.unwrap();
    assert_eq!(config.model, None);
    assert_eq!(config.thinking_option_id, None);
    manager.close_all().await.unwrap();
}
