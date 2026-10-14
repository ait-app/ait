use super::*;
#[test]
fn missing_atomic_roundtrip_and_invalid_version() {
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .canonicalize()
        .unwrap()
        .join("schedules/data.json");
    let mut store = FileStore::new(path.clone());
    assert!(store.load().unwrap().is_empty());
    store.save(&[]).unwrap();
    assert!(store.load().unwrap().is_empty());
    fs::write(&path, r#"{"version":3,"schedules":[]}"#).unwrap();
    assert_eq!(store.load().unwrap_err(), Error::Storage);
    fs::write(&path, "broken").unwrap();
    assert_eq!(store.load().unwrap_err(), Error::Storage);
}
#[cfg(unix)]
#[test]
fn refuses_symlink_targets_and_keeps_private_mode() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap().join("data.json");
    let mut store = FileStore::new(path.clone());
    store.save(&[]).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let link = path.with_extension("link");
    symlink(&path, &link).unwrap();
    let mut bad = FileStore::new(link);
    assert!(bad.load().is_err());
    assert!(bad.save(&[]).is_err());
    assert!(store.load().unwrap().is_empty());
}
#[test]
fn rejects_parent_traversal_and_failed_replace() {
    let root = tempfile::tempdir().unwrap();
    let mut store = FileStore::new(root.path().join("../data"));
    assert!(store.save(&[]).is_err());
    let mut store = FileStore::new(root.path().canonicalize().unwrap());
    assert!(store.save(&[]).is_err());
}

#[test]
fn version_one_records_load_without_guessing_ownership_and_save_as_version_two() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap().join("schedules.json");
    let document = serde_json::json!({"version":1,"schedules":[{
        "id":"11111111-1111-4111-8111-111111111111","name":null,"prompt":"old task",
        "cadence":{"type":"every","everyMs":60_000},"target":{"type":"agent","agentId":"22222222-2222-4222-8222-222222222222"},
        "status":"paused","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","nextRunAt":null,"lastRunAt":null,"pausedAt":null,"expiresAt":null,"maxRuns":null,"runs":[]
    }]});
    fs::write(&path, document.to_string()).unwrap();
    let mut store = FileStore::new(path.clone());
    let records = store.load().unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].workspace_id.is_none());
    assert!(records[0].start_at.is_none());
    store.save(&records).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["version"], 2);
    assert!(saved["schedules"][0]["workspaceId"].is_null());
    assert_eq!(store.load().unwrap()[0].prompt, "old task");
}
