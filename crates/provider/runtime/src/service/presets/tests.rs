use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use model::storage::daemon_config::DaemonConfigStore;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use serde_json::{Value, json};

use super::PresetService;
use provider_core::ports::preset::{Preset, PresetBackend};

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
        .save("claude", cwd, None, &preset("team", "opus"), None)
        .await
        .unwrap();
    service
        .save("claude", cwd, None, &preset("fast", "sonnet"), None)
        .await
        .unwrap();
    // Same name replaces instead of appending.
    service
        .save("claude", cwd, None, &preset("team", "claude-opus-5"), None)
        .await
        .unwrap();
    let listed = service.list("claude", cwd, None).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed[0].source,
        provider_core::ports::preset::PresetSource::Ait
    );
    assert!(listed[0].editable);
    assert_eq!(listed[0].preset.model, "claude-opus-5");

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
    assert!(service.save("claude", cwd, None, &bad, None).await.is_err());
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
        .save("claude", cwd, None, &preset("team", "claude-opus-5"), None)
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
        .save("claude", cwd, None, &preset("team", "claude-opus-5"), None)
        .await
        .unwrap();
    service
        .save("omp", cwd, None, &preset("leaked", "futureppo/model"), None)
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

#[tokio::test]
async fn renaming_replaces_the_named_record_and_refuses_an_occupied_name() {
    let root = tempfile::tempdir().unwrap();
    let (service, _store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    for name in ["alpha", "beta"] {
        service
            .save("claude", cwd, None, &preset(name, "model/a"), None)
            .await
            .unwrap();
    }

    // Renaming alpha to gamma must move it, not create a third record.
    service
        .save(
            "claude",
            cwd,
            None,
            &preset("gamma", "model/b"),
            Some("alpha"),
        )
        .await
        .unwrap();
    let listed = service.list("claude", cwd, None).await.unwrap();
    let mut names: Vec<&str> = listed
        .iter()
        .map(|preset| preset.preset.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["beta", "gamma"]);
    let renamed = listed
        .iter()
        .find(|preset| preset.preset.name == "gamma")
        .unwrap();
    assert_eq!(renamed.preset.model, "model/b");

    // Renaming beta onto gamma would destroy an existing preset, so it is refused.
    assert!(
        service
            .save(
                "claude",
                cwd,
                None,
                &preset("gamma", "model/c"),
                Some("beta")
            )
            .await
            .is_err()
    );
    let listed = service.list("claude", cwd, None).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed
            .iter()
            .find(|preset| preset.preset.name == "gamma")
            .unwrap()
            .preset
            .model,
        "model/b",
        "the refused rename left the target untouched"
    );
}

#[tokio::test]
async fn internal_patch_markers_never_reach_a_client_or_a_store() {
    let root = tempfile::tempdir().unwrap();
    let (service, _store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    // A record echoed back from a native list carries its private overlay markers.
    let mut echoed = preset("team", "model/a");
    echoed
        .feature_values
        .insert("modelPreset".to_owned(), json!(null));
    echoed
        .feature_values
        .insert("modelPresetThinking".to_owned(), json!("auto"));
    echoed
        .feature_values
        .insert("modelRole.advisor".to_owned(), json!("model/b"));

    service
        .save("claude", cwd, None, &echoed, None)
        .await
        .expect("an internal marker must not make an otherwise valid preset unsavable");
    let stored = service.list("claude", cwd, None).await.unwrap();
    let values = &stored[0].preset.feature_values;
    assert!(!values.contains_key("modelPreset"));
    assert!(!values.contains_key("modelPresetThinking"));
    assert_eq!(values.get("modelRole.advisor"), Some(&json!("model/b")));
}

/// A configuration store whose reads take long enough for two writers to overlap.
///
/// The real store reads and writes in separate critical sections, so two concurrent saves can
/// both observe the list from before either write. Widening that window here makes the lost
/// update deterministic instead of a rare timing accident.
#[derive(Debug, Clone)]
struct SlowStore {
    inner: Arc<std::sync::Mutex<Value>>,
}

impl DaemonConfigStore for SlowStore {
    fn get(&self) -> Result<Value, domain::storage::daemon_config::DaemonConfigStoreError> {
        std::thread::sleep(std::time::Duration::from_millis(50));
        Ok(self.inner.lock().unwrap().clone())
    }

    fn patch(
        &self,
        patch: &Value,
    ) -> Result<Value, domain::storage::daemon_config::DaemonConfigStoreError> {
        let mut state = self.inner.lock().unwrap();
        let providers = state["providers"]
            .as_object_mut()
            .ok_or(domain::storage::daemon_config::DaemonConfigStoreError::Invalid)?;
        let patched = patch["providers"]
            .as_object()
            .ok_or(domain::storage::daemon_config::DaemonConfigStoreError::Invalid)?;
        for (provider, entry) in patched {
            providers.insert(provider.clone(), entry.clone());
        }
        Ok(state.clone())
    }

    fn reload(
        &self,
    ) -> Result<
        domain::storage::daemon_config::DaemonConfigReload,
        domain::storage::daemon_config::DaemonConfigStoreError,
    > {
        Err(domain::storage::daemon_config::DaemonConfigStoreError::Invalid)
    }
}

#[test]
fn concurrent_creates_all_survive_without_lost_updates() {
    let store = SlowStore {
        inner: Arc::new(std::sync::Mutex::new(json!({"providers":{}}))),
    };
    let service = PresetService::new(Arc::new(store));
    let names = ["alpha", "beta"];
    // Each save reads the whole list and writes it back, so the two writers must overlap to make
    // the lost update reproducible. Two operating-system threads overlap for certain, whereas
    // `save` has no yield point that would let two spawned tasks interleave inside one runtime.
    std::thread::scope(|scope| {
        let handles: Vec<_> = names
            .iter()
            .map(|name| {
                let service = service.clone();
                scope.spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .build()
                        .unwrap()
                        .block_on(service.save("claude", ".", None, &preset(name, "model/a"), None))
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .unwrap()
                .expect("a concurrent create must not fail");
        }
    });

    let listed = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(service.list("claude", ".", None))
        .unwrap();
    let mut stored: Vec<&str> = listed
        .iter()
        .map(|preset| preset.preset.name.as_str())
        .collect();
    stored.sort_unstable();
    assert_eq!(
        stored, names,
        "a lost update dropped a concurrently created preset"
    );
}

#[tokio::test]
async fn role_assignments_survive_a_reopen_at_full_fidelity() {
    let root = tempfile::tempdir().unwrap();
    let (service, store) = service(&root);
    let cwd = root.path().to_str().unwrap();
    let mut assigned = preset("director", "gpt-5");
    assigned.thinking_option_id = Some("high".to_owned());
    assigned.feature_values = std::collections::BTreeMap::from([
        (
            "modelRole.advisor".to_owned(),
            serde_json::json!("gpt-5-mini"),
        ),
        ("modelRole.worker".to_owned(), serde_json::json!("gpt-5")),
    ]);
    service
        .save("codex", cwd, None, &assigned, None)
        .await
        .unwrap();

    // Reopening over the same file must return the whole configuration, not just the model:
    // the role assignments are what the preset exists to carry.
    let reopened = PresetService::new(store.clone());
    let listed = reopened.list("codex", cwd, None).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].preset.model, "gpt-5");
    assert_eq!(listed[0].preset.thinking_option_id.as_deref(), Some("high"));
    assert_eq!(
        listed[0].preset.feature_values.get("modelRole.advisor"),
        Some(&serde_json::json!("gpt-5-mini"))
    );
    assert_eq!(
        listed[0].preset.feature_values.get("modelRole.worker"),
        Some(&serde_json::json!("gpt-5"))
    );
}
