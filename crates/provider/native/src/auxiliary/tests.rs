use super::*;

#[test]
fn request_paths_are_bounded_and_cannot_escape_the_auxiliary_directory() {
    let root = tempfile::tempdir().unwrap();
    let path = path(root.path(), "../../escape / with unicode 问题");
    assert_eq!(path.parent(), Some(root.path()));
    assert_eq!(
        path.file_name().unwrap().to_str().unwrap().len(),
        "request-".len() + 64 + ".json".len()
    );
}

#[test]
fn live_or_unknown_process_ownership_is_never_signalled_or_declared_gone() {
    let root = tempfile::tempdir().unwrap();
    let path = path(root.path(), "request");
    create(
        &path,
        "request",
        "topic",
        "thread",
        u32::try_from(nix::unistd::getpgrp().as_raw()).unwrap(),
    )
    .unwrap();
    assert!(reconcile(&path, "request").is_err());
    assert!(path.exists());
    assert!(reconcile(&path, "wrong-request").is_err());
    assert!(path.exists());
    remove(&path).unwrap();
    remove(&path).unwrap();
    reconcile(&path, "request").unwrap();
}

#[test]
fn marker_is_private_and_immutable_and_contains_no_prompt_material() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = path(root.path(), "request");
    create(&path, "request", "topic", "thread", 123).unwrap();
    assert!(create(&path, "request", "other", "other", 456).is_err());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["request_id"], "request");
    assert_eq!(value["process_group"], 123);
}

#[test]
fn confirmed_reaped_group_releases_its_marker_without_replaying_work() {
    use std::os::unix::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .process_group(0)
        .spawn()
        .unwrap();
    let group = child.id();
    child.wait().unwrap();
    let path = path(root.path(), "request");
    create(&path, "request", "topic", "thread", group).unwrap();
    reconcile(&path, "request").unwrap();
    assert!(!path.exists());
}
