use super::*;

#[test]
fn service_record_is_private_and_replaced_with_the_new_instance() {
    let directory = tempfile::tempdir().unwrap();
    let token = SecretString::from("private-test-token-at-least-32-characters");
    let address = "127.0.0.1:7316".parse().unwrap();
    let first = InstanceLease::acquire(directory.path()).unwrap();
    publish(directory.path(), address, &first, &token).unwrap();
    let path = directory.path().join("service-connection.json");
    let read =
        || serde_json::from_slice::<serde_json::Value>(&std::fs::read(&path).unwrap()).unwrap();
    let original = read();
    assert_eq!(original["managementMode"], "external");
    assert_eq!(original["token"], token.expose_secret());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    drop(first);
    let second = InstanceLease::acquire(directory.path()).unwrap();
    publish(directory.path(), address, &second, &token).unwrap();
    let updated = read();
    assert_eq!(original["serverId"], updated["serverId"]);
    assert_ne!(original["instanceId"], updated["instanceId"]);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 3);
}
