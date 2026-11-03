use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

#[test]
fn concurrent_startups_reuse_one_private_credential() {
    let directory = tempfile::tempdir().unwrap();
    let tokens = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| load_or_create(directory.path()).unwrap()))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    let saved = load_or_create(directory.path()).unwrap();
    assert_eq!(saved.expose_secret().len(), 64);
    api::validate_token(saved.expose_secret()).unwrap();
    for token in tokens {
        assert_eq!(token.expose_secret(), saved.expose_secret());
    }
    assert_eq!(
        std::fs::metadata(directory.path().join("local-api.token"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn invalid_or_exposed_storage_is_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.token");
    let token = load_or_create(directory.path()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load_or_create(directory.path()).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        token.expose_secret()
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    for invalid in [String::new(), "short".to_owned(), "x".repeat(257)] {
        std::fs::write(&path, &invalid).unwrap();
        assert!(load_or_create(directory.path()).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
}

#[test]
fn rejects_symlinks_and_directories() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.token");
    let target = directory.path().join("missing");
    symlink(&target, &path).unwrap();
    assert!(load_or_create(directory.path()).is_err());
    assert!(!target.exists());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(load_or_create(directory.path()).is_err());
}

#[test]
fn hand_edited_credentials_tolerate_a_line_ending_but_not_group_access() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("local-api.token");
    let token = load_or_create(directory.path()).unwrap();
    for ending in ["\n", "\r\n"] {
        std::fs::write(&path, format!("{}{ending}", token.expose_secret())).unwrap();
        assert_eq!(
            load_or_create(directory.path()).unwrap().expose_secret(),
            token.expose_secret()
        );
    }
    let longest = "x".repeat(256);
    std::fs::write(&path, format!("{longest}\r\n")).unwrap();
    assert_eq!(
        load_or_create(directory.path()).unwrap().expose_secret(),
        longest
    );
    std::fs::write(&path, format!("{longest}\nextra")).unwrap();
    assert!(load_or_create(directory.path()).is_err());
    std::fs::write(&path, token.expose_secret()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(load_or_create(directory.path()).is_err());
}
