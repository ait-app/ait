use serde_json::json;

use super::*;

#[test]
fn reads_missing_and_existing_documents_and_rejects_invalid_json() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_str().unwrap();
    let store = LocalProjectConfigStore;
    assert_eq!(
        store.read(root).unwrap(),
        ProjectConfigDocument {
            config: None,
            revision: None
        }
    );
    std::fs::write(
        fixture.path().join(PROJECT_CONFIG_FILE_NAME),
        "{\"future\":true}",
    )
    .unwrap();
    let document = store.read(root).unwrap();
    assert_eq!(document.config, Some(json!({"future":true})));
    assert!(document.revision.is_some());
    std::fs::write(fixture.path().join(PROJECT_CONFIG_FILE_NAME), "invalid").unwrap();
    assert_eq!(store.read(root), Err(ProjectConfigStoreError::Invalid));
}

#[test]
fn writes_atomically_and_rejects_stale_or_absent_revision_mismatches() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_str().unwrap();
    let store = LocalProjectConfigStore;
    let first = store.write(root, &json!({"one":1}), None).unwrap();
    let ProjectConfigWrite::Written { revision, .. } = first else {
        panic!("expected write")
    };
    assert_eq!(
        std::fs::read_to_string(fixture.path().join(PROJECT_CONFIG_FILE_NAME)).unwrap(),
        "{\n  \"one\": 1\n}\n"
    );
    assert!(matches!(
        store.write(root, &json!({"two":2}), None).unwrap(),
        ProjectConfigWrite::Stale {
            current_revision: Some(_)
        }
    ));
    assert_eq!(store.read(root).unwrap().config, Some(json!({"one":1})));
    let second = store
        .write(root, &json!({"two":2}), Some(revision))
        .unwrap();
    assert!(matches!(second, ProjectConfigWrite::Written { .. }));
}

#[test]
fn reads_legacy_config_and_migrates_writes_without_overwriting_the_original() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_str().unwrap();
    let legacy = fixture.path().join(LEGACY_PROJECT_CONFIG_FILE_NAME);
    std::fs::write(&legacy, "{\"legacy\":true}").unwrap();
    let store = LocalProjectConfigStore;
    let document = store.read(root).unwrap();
    assert_eq!(document.config, Some(json!({"legacy":true})));
    assert!(matches!(
        store
            .write(root, &json!({"ait":true}), document.revision)
            .unwrap(),
        ProjectConfigWrite::Written { .. }
    ));
    assert_eq!(store.read(root).unwrap().config, Some(json!({"ait":true})));
    assert_eq!(
        std::fs::read_to_string(&legacy).unwrap(),
        "{\"legacy\":true}"
    );
    std::fs::write(fixture.path().join(PROJECT_CONFIG_FILE_NAME), "invalid").unwrap();
    assert_eq!(store.read(root), Err(ProjectConfigStoreError::Invalid));
}

#[test]
fn config_path_prefers_ait_and_falls_back_to_legacy_only_when_absent() {
    let fixture = tempfile::tempdir().unwrap();
    let store = LocalProjectConfigStore;
    let legacy = fixture.path().join(LEGACY_PROJECT_CONFIG_FILE_NAME);
    assert_eq!(store.config_path(fixture.path()).unwrap(), legacy);
    std::fs::write(&legacy, "{}").unwrap();
    assert_eq!(store.config_path(fixture.path()).unwrap(), legacy);
    let preferred = fixture.path().join(PROJECT_CONFIG_FILE_NAME);
    std::fs::write(&preferred, "invalid").unwrap();
    assert_eq!(store.config_path(fixture.path()).unwrap(), preferred);
}

#[cfg(unix)]
#[test]
fn config_path_reports_uninspectable_roots_without_selecting_legacy() {
    let fixture = tempfile::tempdir().unwrap();
    let blocker = fixture.path().join("not-a-directory");
    std::fs::write(&blocker, "file").unwrap();
    assert!(LocalProjectConfigStore.config_path(&blocker).is_err());
}
