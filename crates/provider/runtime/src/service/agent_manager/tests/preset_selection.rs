use super::*;
use domain::agent_runtime::{ModelPresetReference, StoredAgentConfig};
use provider_core::ports::preset::Preset;
use provider_core::protocol::agent_config::ConfigPatch;
use serde_json::json;
use std::collections::BTreeMap;

fn preset(name: &str, model: &str, role: &str) -> Preset {
    Preset {
        name: name.into(),
        model: model.into(),
        thinking_option_id: Some("high".into()),
        feature_values: BTreeMap::from([
            ("modelRole.task".into(), json!(role)),
            ("modelFallback.default".into(), json!(["provider/backup"])),
        ]),
    }
}
fn reference(name: &str) -> ModelPresetReference {
    ModelPresetReference {
        name: name.into(),
        source: "global".into(),
        cwd: "/repo".into(),
        agent_cwd: "/repo".into(),
    }
}
fn patch(value: Value) -> ConfigPatch {
    serde_json::from_value(value).unwrap()
}

#[test]
fn preset_selection_is_explicit_even_when_models_match_and_restores_independent_roles() {
    let initial = StoredAgentConfig {
        model: Some("model".into()),
        thinking_option_id: Some("low".into()),
        feature_values: Some(BTreeMap::from([
            ("modelRole.task".into(), json!("user/task")),
            ("fastMode".into(), json!(true)),
        ])),
        ..StoredAgentConfig::default()
    };
    let (first, first_state) = super::super::preset_selection::select(
        &initial,
        None,
        reference("A"),
        &preset("A", "model", "preset/a"),
    );
    let (second, state) = super::super::preset_selection::select(
        &first,
        Some(&first_state),
        reference("B"),
        &preset("B", "model", "preset/b"),
    );
    assert_eq!(state.selected.as_ref().unwrap().name, "B");
    let (plain, marker) = super::super::preset_selection::manual(
        &second,
        Some(&state),
        &patch(json!({"modelId":"model"})),
    );
    assert!(marker.is_none());
    assert_eq!(plain.thinking_option_id.as_deref(), Some("low"));
    let features = plain.feature_values.unwrap();
    assert_eq!(features["modelRole.task"], "user/task");
    assert_eq!(features["fastMode"], true);
    assert!(!features.contains_key("modelFallback.default"));
}

#[test]
fn preset_custom_role_is_preserved_and_remaining_owned_fields_are_restored() {
    let initial = StoredAgentConfig::default();
    let (selected, state) = super::super::preset_selection::select(
        &initial,
        None,
        reference("A"),
        &preset("A", "model", "preset/a"),
    );
    let (custom, state) = super::super::preset_selection::manual(
        &selected,
        Some(&state),
        &patch(json!({"featureValues":{"modelRole.task":"user/edited"}})),
    );
    assert!(state.is_none());
    let values = custom.feature_values.unwrap();
    assert_eq!(values["modelRole.task"], "user/edited");
    assert!(!values.contains_key("modelFallback.default"));
}

#[tokio::test]
async fn stale_preset_lookup_does_not_overwrite_newer_or_aba_configuration() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(
        persistence::storage::daemon_config::FileDaemonConfigStore::with_defaults(
            root.path().join("config.json"),
        ),
    );
    let service = crate::service::presets::PresetService::new(store);
    let (manager, registry, _) = make_manager();
    let manager = manager.with_presets(service.clone());
    registry.upsert(&stored_record()).unwrap();
    service
        .save(
            "codex",
            "/tmp/project",
            None,
            &preset("A", "model", "preset/a"),
            None,
        )
        .await
        .unwrap();
    let stale = manager
        .prepare_preset("agent-1", "A", "ait", None)
        .unwrap()
        .await
        .unwrap()
        .unwrap();
    manager
        .configure("agent-1", &patch(json!({"modelId":"other"})))
        .await
        .unwrap();
    assert_eq!(manager.commit_preset(&stale), Err(ErrorCode::CatalogBusy));
    let old_model = stored_record().config.unwrap().model.unwrap();
    manager
        .configure("agent-1", &patch(json!({"modelId":old_model})))
        .await
        .unwrap();
    assert_eq!(manager.commit_preset(&stale), Err(ErrorCode::CatalogBusy));
    let current = registry.get("agent-1").unwrap().unwrap();
    assert_eq!(current.config_revision, 2);
    assert!(current.model_preset.is_none());
}
