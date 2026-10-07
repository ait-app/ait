use std::fs;

use file::storage::push::FileTokenStore;
use model::storage::push::TokenStore;
use serde_json::json;

use crate::service::push::PushTokens;

#[test]
fn private_atomic_file_reloads_and_migrates_legacy_tokens() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("push-tokens.json");
    let store = FileTokenStore::new(path.clone());
    assert_eq!(store.load().unwrap(), json!({}));
    store.save(&json!({"tokens":["legacy"]})).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        // Loading alone repairs permissions; migration must not mask a missing repair.
        store.load().unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let mut service = PushTokens::open(Box::new(store), 0).unwrap();
    assert_eq!(service.active(0), ["legacy"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    service.revoke("legacy").unwrap();
    assert_eq!(
        FileTokenStore::new(path).load().unwrap(),
        json!({"subscriptions":[]})
    );
}
