use super::*;

#[test]
fn native_handles_must_have_a_matching_provider_and_bounded_identity() {
    let mut handle = AgentPersistenceHandle {
        provider: "pi".into(),
        session_id: "native".into(),
        native_handle: None,
        metadata: None,
    };
    assert_eq!(validate_persistence(&handle, "pi"), Ok(()));
    assert_eq!(
        validate_persistence(&handle, "codex"),
        Err(model::ErrorCode::InvalidMessage)
    );
    handle.session_id.clear();
    assert_eq!(
        validate_persistence(&handle, "pi"),
        Err(model::ErrorCode::InvalidMessage)
    );
    handle.session_id = "x".repeat(256 * 1024);
    assert_eq!(
        validate_persistence(&handle, "pi"),
        Err(model::ErrorCode::InvalidMessage)
    );
}
