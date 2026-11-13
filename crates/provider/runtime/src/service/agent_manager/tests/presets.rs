//! Preset requests route to the store that owns the provider, not to a fixed table.

use std::sync::Arc;

use model::ErrorCode;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use serde_json::json;

use super::*;

/// A manager wired to a real file-backed config store, so a save is observable after a restart.
fn manager_with_presets(root: &tempfile::TempDir) -> AgentManager {
    let store = Arc::new(FileDaemonConfigStore::with_defaults(
        root.path().join("config.json"),
    ));
    AgentManager::new(Box::new(MemoryRegistry::default()))
        .with_presets(crate::service::presets::PresetService::new(store))
}

#[tokio::test]
async fn list_without_an_installed_client_reports_an_empty_ait_store() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager_with_presets(&root);
    let listed = manager
        .presets(
            "provider.presets.list.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await
        .unwrap();
    assert_eq!(listed["provider"], "claude");
    assert_eq!(listed["presets"], json!([]));
    assert!(listed["error"].is_null());
}

#[tokio::test]
async fn a_save_is_rejected_before_storage_when_the_name_is_unusable() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager_with_presets(&root);
    let saved = manager
        .presets(
            "provider.presets.save.request",
            "claude",
            &json!({"provider":"claude","preset":{"name":"","model":"claude-x"}}),
        )
        .await
        .unwrap();
    assert!(saved["saved"].is_null(), "{saved}");
    assert!(
        saved["error"]
            .as_str()
            .is_some_and(|error| error.contains("name")),
        "{saved}"
    );
}

#[tokio::test]
async fn a_saved_preset_is_listed_back_and_can_be_deleted() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager_with_presets(&root);
    let saved = manager
        .presets(
            "provider.presets.save.request",
            "claude",
            &json!({"provider":"claude","preset":{
                "name":"fast","model":"claude-sonnet",
                "featureValues":{"modelRole.advisor":"claude-opus"}}}),
        )
        .await
        .unwrap();
    assert_eq!(saved["saved"], "fast", "{saved}");

    let listed = manager
        .presets(
            "provider.presets.list.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await
        .unwrap();
    let presets = listed["presets"].as_array().unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0]["name"], "fast");
    assert_eq!(presets[0]["model"], "claude-sonnet");
    assert_eq!(
        presets[0]["featureValues"]["modelRole.advisor"],
        "claude-opus"
    );
    assert_eq!(presets[0]["source"], "ait");
    assert_eq!(presets[0]["editable"], true);

    let deleted = manager
        .presets(
            "provider.presets.delete.request",
            "claude",
            &json!({"provider":"claude","name":"fast"}),
        )
        .await
        .unwrap();
    assert_eq!(deleted["deleted"], "fast", "{deleted}");
    let listed = manager
        .presets(
            "provider.presets.list.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await
        .unwrap();
    assert_eq!(listed["presets"], json!([]));
}

#[tokio::test]
async fn describe_reports_an_unavailable_store_when_the_provider_is_not_installed() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager_with_presets(&root);
    let described = manager
        .presets(
            "provider.presets.describe.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await
        .unwrap();
    // No client is registered, so the descriptor must withhold writes and carry the reason
    // rather than pretending an Ait store could be used.
    assert_eq!(described["descriptor"]["backend"], "ait");
    assert_eq!(described["descriptor"]["canCreate"], false);
    assert!(described["descriptor"]["reason"].is_string());
}

#[tokio::test]
async fn an_unknown_preset_method_is_not_found_instead_of_silently_succeeding() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager_with_presets(&root);
    let result = manager
        .presets(
            "provider.presets.unknown.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await;
    assert_eq!(result.unwrap_err(), ErrorCode::MethodNotFound);
}

#[tokio::test]
async fn a_manager_without_the_service_reports_unsupported_capability() {
    let manager = AgentManager::new(Box::new(MemoryRegistry::default()));
    let result = manager
        .presets(
            "provider.presets.list.request",
            "claude",
            &json!({"provider":"claude"}),
        )
        .await;
    assert_eq!(result.unwrap_err(), ErrorCode::UnsupportedCapability);
}
