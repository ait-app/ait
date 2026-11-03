use super::*;

#[test]
fn delivered_archives_decorate_only_the_matching_native_command() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::new(directory.path().join("exports"));
    let mut item = NativeItem {
        key: "native:stable".into(),
        turn_id: None,
        timestamp: "time".into(),
        item: json!({"type":"provider_command","id":"dsh:session:command:export-1","command":"export","status":"success","text":"Requested"}),
    };
    store.decorate(&mut item).unwrap();
    assert!(item.item.get("file").is_none());
    let target = store.directory("dsh:session:command:export-1");
    private_directory(&target).unwrap();
    std::fs::write(target.join("session.zip"), b"archive bytes").unwrap();
    store.decorate(&mut item).unwrap();
    assert_eq!(item.key, "native:stable");
    assert_eq!(item.item["file"]["directory"], target.to_str().unwrap());
    assert_eq!(item.item["file"]["size"], 13);
    item.item.as_object_mut().unwrap().remove("file");
    item.item["id"] = json!("dsh:other:command:export-1");
    store.decorate(&mut item).unwrap();
    assert!(item.item.get("file").is_none());
}

#[cfg(unix)]
#[test]
fn archive_storage_rejects_public_directories_and_symbolic_targets() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_directory(directory.path()).is_err());
    let link = directory.path().join("link");
    symlink(directory.path(), &link).unwrap();
    assert!(private_directory(&link).is_err());
}
