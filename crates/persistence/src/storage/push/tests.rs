use super::*;
use serde_json::json;

#[test]
fn invalid_documents_and_nonfiles_are_rejected_without_overwrite() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tokens");
    fs::write(&path, "corrupt").unwrap();
    assert_eq!(
        FileTokenStore::new(path.clone()).load(),
        Err(PushError::Invalid)
    );
    let store = FileTokenStore::new(root.path().to_path_buf());
    assert_eq!(store.load(), Err(PushError::Invalid));
    assert_eq!(store.save(&json!({})), Err(PushError::Invalid));
    #[cfg(unix)]
    {
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let store = FileTokenStore::new(link);
        assert_eq!(store.load(), Err(PushError::Invalid));
        assert_eq!(store.save(&json!({})), Err(PushError::Invalid));
        assert_eq!(fs::read_to_string(path).unwrap(), "corrupt");
    }
}

#[cfg(unix)]
#[test]
fn save_keeps_private_modes_and_load_tightens_existing_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("push").join("tokens");
    let store = FileTokenStore::new(path.clone());
    assert_eq!(store.load(), Ok(json!({})));
    store.save(&json!({"token":"value"})).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(fs::read_to_string(&path).unwrap().ends_with('\n'));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(store.load(), Ok(json!({"token":"value"})));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn oversized_documents_report_capacity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tokens");
    fs::File::create(&path)
        .unwrap()
        .set_len(MAX_DOCUMENT_BYTES + 1)
        .unwrap();
    assert_eq!(FileTokenStore::new(path).load(), Err(PushError::Capacity));
}
