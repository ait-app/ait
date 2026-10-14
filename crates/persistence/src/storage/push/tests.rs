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
