use super::*;

use metadata::ports::daemon::{DaemonConfigReload, DaemonConfigStoreError};
use metadata::storage::daemon_config::FileDaemonConfigStore;
use serde_json::json;

#[test]
fn configuration_reads_live_preferences_and_repository_styles_with_legacy_fallback() {
    let root = tempfile::tempdir().unwrap();
    let store = Arc::new(FileDaemonConfigStore::with_defaults(
        root.path().join("config.json"),
    ));
    let configuration = Configuration(store.clone());
    store
        .patch(&json!({"metadataGeneration":{"providers":[{"provider":"codex","model":"chosen"}]}}))
        .unwrap();
    assert_eq!(
        configuration.current().unwrap()["metadataGeneration"]["providers"][0]["model"],
        "chosen"
    );
    store
        .patch(&json!({"metadataGeneration":{"providers":[]}}))
        .unwrap();
    assert_eq!(
        configuration.current().unwrap()["metadataGeneration"]["providers"],
        json!([])
    );
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let nested = root.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let cwd = nested.to_str().unwrap();
    assert_eq!(configuration.project(cwd), Value::Null);
    std::fs::write(
        root.path().join("paseo.json"),
        json!({"metadataGeneration":{"title":{"instructions":"Legacy style"}}}).to_string(),
    )
    .unwrap();
    assert_eq!(
        configuration.project(cwd)["metadataGeneration"]["title"]["instructions"],
        "Legacy style"
    );
    std::fs::write(
        root.path().join("ait.json"),
        json!({"metadataGeneration":{"title":{"instructions":"Current style"}}}).to_string(),
    )
    .unwrap();
    assert_eq!(
        configuration.project(cwd)["metadataGeneration"]["title"]["instructions"],
        "Current style"
    );
    std::fs::write(root.path().join("ait.json"), "invalid").unwrap();
    assert_eq!(configuration.project(cwd), Value::Null);
    assert_eq!(
        configuration.project("/missing-summary-directory"),
        Value::Null
    );
}

#[derive(Debug)]
struct Unavailable;

impl DaemonConfigStore for Unavailable {
    fn get(&self) -> Result<Value, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Io)
    }
    fn patch(&self, _: &Value) -> Result<Value, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Io)
    }
    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Io)
    }
}

#[test]
fn configuration_storage_failures_become_safe_summary_errors() {
    assert_eq!(
        Configuration(Arc::new(Unavailable)).current(),
        Err(SummaryError::Unavailable)
    );
}
