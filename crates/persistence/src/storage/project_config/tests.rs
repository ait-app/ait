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

#[test]
fn oversized_documents_are_rejected_without_parsing() {
    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join(PROJECT_CONFIG_FILE_NAME);
    File::create(&path)
        .unwrap()
        .set_len(u64::from(MAX_CONFIG_BYTES) + 1)
        .unwrap();
    assert_eq!(
        LocalProjectConfigStore.read(fixture.path().to_str().unwrap()),
        Err(ProjectConfigStoreError::Invalid)
    );
}

#[cfg(unix)]
#[test]
fn a_written_project_config_keeps_the_mode_of_the_file_it_replaces() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_str().unwrap();
    let store = LocalProjectConfigStore;
    let path = fixture.path().join(PROJECT_CONFIG_FILE_NAME);
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    store.write(root, &json!({"future":true}), None).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644);
}

#[test]
fn a_document_beyond_the_read_limit_is_refused_instead_of_persisted() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().to_str().unwrap();
    let store = LocalProjectConfigStore;
    let oversized = json!({"note":"x".repeat(usize::try_from(MAX_CONFIG_BYTES).unwrap() + 1)});
    assert_eq!(
        store.write(root, &oversized, None),
        Err(ProjectConfigStoreError::Write)
    );
    // The persisted project configuration stays within the limit the read path enforces.
    assert_eq!(store.read(root).unwrap().config, None);
}
