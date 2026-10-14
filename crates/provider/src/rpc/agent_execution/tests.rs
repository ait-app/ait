use super::*;

#[test]
fn rejected_session_operations_are_invalid_requests_not_provider_io_failures() {
    assert_eq!(
        map_manager(&AgentManagerError::SessionRejected),
        ErrorCode::InvalidMessage
    );
    assert_eq!(map_manager(&AgentManagerError::Session), ErrorCode::AgentIo);
}
