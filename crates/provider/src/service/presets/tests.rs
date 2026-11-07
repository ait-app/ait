use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use model::storage::daemon_config::DaemonConfigStore;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use serde_json::json;

use super::PresetService;
use crate::ports::preset::{Preset, PresetBackend};

fn service(root: &tempfile::TempDir) -> (PresetService, Arc<FileDaemonConfigStore>) {
    let store = Arc::new(FileDaemonConfigStore::with_defaults(
        root.path().join("config.json"),
    ));
    (PresetService::new(store.clone()), store)
}

fn preset(name: &str, model: &str) -> Preset {
    Preset {
        name: name.to_owned(),
        model: model.to_owned(),
        thinking_option_id: None,
        feature_values: std::collections::BTreeMap::new(),
    }
}

#[tokio::test]
async fn ait_backend_creates_lists_replaces_and_deletes_across_restarts() {
    let root = tempfile::tempdir().unwrap();
    let (service, store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    assert_eq!(
        service.describe(cwd, None).await.backend,
        PresetBackend::Ait
    );
    service
        .save("claude", cwd, None, &preset("team", "opus"))
        .await
        .unwrap();
    service
        .save("claude", cwd, None, &preset("fast", "sonnet"))
        .await
        .unwrap();
    // Same name replaces instead of appending.
    service
        .save("claude", cwd, None, &preset("team", "claude-opus-5"))
        .await
        .unwrap();
    let listed = service.list("claude", cwd, None).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].model, "claude-opus-5");

    // A second service over the same file sees the persisted list.
    let reopened = PresetService::new(store.clone());
    assert_eq!(reopened.list("claude", cwd, None).await.unwrap().len(), 2);
    assert!(reopened.list("codex", cwd, None).await.unwrap().is_empty());

    service.delete("claude", cwd, None, "team").await.unwrap();
    assert_eq!(service.list("claude", cwd, None).await.unwrap().len(), 1);
    assert!(service.delete("claude", cwd, None, "team").await.is_err());
}

#[tokio::test]
async fn an_invalid_preset_is_rejected_before_storage() {
    let root = tempfile::tempdir().unwrap();
    let (service, store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    let mut bad = preset("team", "opus");
    bad.feature_values.insert("1bad".to_owned(), json!(true));
    assert!(service.save("claude", cwd, None, &bad).await.is_err());
    assert!(
        store.get().unwrap()["providers"]["claude"].is_null(),
        "nothing was persisted"
    );
}

#[tokio::test]
async fn synthesis_prefers_a_native_feature_and_otherwise_uses_stored_presets() {
    let root = tempfile::tempdir().unwrap();
    let (service, _store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    let current = StoredAgentConfig::default();
    service
        .save("claude", cwd, None, &preset("team", "claude-opus-5"))
        .await
        .unwrap();

    let mut features = vec![json!({"id":"fast_mode","type":"toggle","value":false})];
    service.apply("claude", false, &mut features, &current);
    assert_eq!(features[0]["id"], "modelPreset");
    assert_eq!(features[1]["id"], "fast_mode");

    // A provider with no stored presets gains nothing.
    let mut plain = vec![json!({"id":"fast_mode","type":"toggle","value":false})];
    service.apply("codex", false, &mut plain, &current);
    assert_eq!(plain.len(), 1);

    // A native feature already present is preserved in place.
    let native = json!({"type":"select","id":"modelPreset","value":"gpt"});
    let mut native_features = vec![native.clone()];
    service.apply("omp", true, &mut native_features, &current);
    assert_eq!(native_features, vec![native]);
}

#[tokio::test]
async fn a_native_store_never_receives_ait_entries_even_when_it_has_none() {
    let root = tempfile::tempdir().unwrap();
    let (service, _store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    let current = StoredAgentConfig::default();
    // Ait has presets for both providers; only the Ait-backed one may gain a select.
    service
        .save("claude", cwd, None, &preset("team", "claude-opus-5"))
        .await
        .unwrap();
    service
        .save("omp", cwd, None, &preset("leaked", "futureppo/model"))
        .await
        .unwrap();

    let mut native = vec![json!({"id":"role","type":"select","value":"x"})];
    service.apply("omp", true, &mut native, &current);
    assert_eq!(
        native,
        vec![json!({"id":"role","type":"select","value":"x"})],
        "a native store must not be shown Ait presets"
    );

    let mut managed = Vec::new();
    service.apply("claude", false, &mut managed, &current);
    assert_eq!(managed.len(), 1);
    assert_eq!(managed[0]["id"], "modelPreset");
}
