use super::*;

#[test]
fn rejected_session_operations_are_invalid_requests_not_provider_io_failures() {
    assert_eq!(
        map_manager(&AgentManagerError::SessionRejected),
        ErrorCode::InvalidMessage
    );
    assert_eq!(map_manager(&AgentManagerError::Session), ErrorCode::AgentIo);
}

#[test]
fn ledger_failures_map_to_a_missing_capability_or_retryable_io() {
    use crate::ports::usage_ledger::UsageLedgerError;

    assert_eq!(
        controls::map_ledger(UsageLedgerError::Unavailable),
        ErrorCode::UnsupportedCapability
    );
    assert_eq!(
        controls::map_ledger(UsageLedgerError::Failed),
        ErrorCode::AgentIo
    );
}
