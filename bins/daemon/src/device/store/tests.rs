use super::*;
use chrono::Utc;
use host_link::Binding;

fn snapshot() -> State {
    let server_id = Uuid::new_v4();
    State {
        machine: Machine {
            server_id,
            display_name: "build's \"linux\"".into(),
            platform: "linux".into(),
            app_version: "test".into(),
        },
        credential: Some(Credential {
            refresh_token: "private-refresh".into(),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
            binding: Binding {
                node_id: Uuid::new_v4(),
                host_id: Uuid::new_v4(),
                server_id,
                grant_id: Uuid::new_v4(),
            },
        }),
        pending: Some(Pending::Refresh(Uuid::new_v4())),
    }
}

#[test]
fn private_state_round_trips_and_holds_an_exclusive_lock() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    assert!(store.load().unwrap().is_none());
    assert_eq!(Store::open(data.path()).unwrap_err(), Error::Conflict);
    let state = snapshot();
    for pending in [
        None,
        state.pending.clone(),
        Some(Pending::Enrollment {
            token: "secret-enrollment".into(),
            request_id: Uuid::new_v4(),
        }),
        Some(Pending::Web {
            device_code: "secret-web".into(),
            user_code: "ABCD-1234-EF56".into(),
            request_id: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::minutes(10),
            interval: 5,
        }),
    ] {
        let mut state = state.clone();
        state.pending = pending;
        store.save(&state).unwrap();
        let restored = store.load().unwrap().unwrap();
        assert_eq!(encode(&state), encode(&restored));
        assert!(!format!("{restored:?}").contains("private-refresh"));
    }
    let path = data.path().join("device/.env");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(data.path().join("device"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_eq!(
        std::fs::read_dir(data.path().join("device"))
            .unwrap()
            .count(),
        2
    );
    store.clear().unwrap();
    store.clear().unwrap();
    assert!(store.load().unwrap().is_none());
    drop(store);
    assert!(Store::open(data.path()).is_ok());
}

#[test]
fn rejects_corrupt_oversize_or_inconsistent_state_without_echoing_it() {
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    for text in [
        "sensitive-garbage".to_owned(),
        "AIT_DEVICE_STATE=invalid".to_owned(),
        "x".repeat(65537),
    ] {
        std::fs::write(data.path().join("device/.env"), text).unwrap();
        assert_eq!(store.load().unwrap_err(), Error::Storage);
    }
    let mut value = encode(&snapshot());
    value["version"] = json!(2);
    assert!(decode(&value).is_err());
    value["version"] = json!(1);
    value["credential"]["binding"]["server_id"] = json!(Uuid::new_v4());
    assert!(decode(&value).is_err());
    value["credential"] = Value::Null;
    value["pending"]["kind"] = json!("unknown");
    assert!(decode(&value).is_err());
}

#[cfg(unix)]
#[test]
fn refuses_symlinks_directories_and_world_readable_secrets() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let data = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), data.path().join("device")).unwrap();
    assert!(Store::open(data.path()).is_err());
    std::fs::remove_file(data.path().join("device")).unwrap();
    let store = Store::open(data.path()).unwrap();
    let path = data.path().join("device/.env");
    symlink(outside.path().join("untouched"), &path).unwrap();
    assert!(store.save(&snapshot()).is_err());
    assert!(store.load().is_err());
    assert!(store.clear().is_err());
    assert!(!outside.path().join("untouched").exists());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(store.save(&snapshot()).is_err());
    std::fs::remove_dir(&path).unwrap();
    store.save(&snapshot()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.load().is_err());
    drop(store);
    std::fs::set_permissions(
        data.path().join("device"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(Store::open(data.path()).is_err());
}
