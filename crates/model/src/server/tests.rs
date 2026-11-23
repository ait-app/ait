use super::*;

#[test]
fn agent_io_message_advises_checking_state_before_retrying() {
    let error = ErrorCode::AgentIo;

    assert_eq!(
        error.message(),
        "Agent operation failed; check the current state before retrying"
    );
    assert!(error.retryable());
    assert_eq!(serde_json::to_value(error).unwrap(), "agent_io");
}

#[test]
fn server_info_preserves_software_and_protocol_versions() {
    let info = crate::tests::runtime().info();

    let serialized = serde_json::to_value(&info).unwrap();
    let restored: ServerInfo = serde_json::from_value(serialized.clone()).unwrap();

    assert_eq!(serialized["version"], crate::BUILD_VERSION);
    assert_eq!(restored, info);
    assert_eq!(restored.protocol, VERSION);
}

#[test]
fn server_info_accepts_older_servers_without_a_software_version() {
    let mut serialized = serde_json::to_value(crate::tests::runtime().info()).unwrap();
    serialized.as_object_mut().unwrap().remove("version");

    let restored: ServerInfo = serde_json::from_value(serialized).unwrap();

    assert_eq!(restored.version, None);
    assert_eq!(restored.protocol, VERSION);
    assert!(
        serde_json::to_value(restored)
            .unwrap()
            .get("version")
            .is_none()
    );
}

mod negotiation;
