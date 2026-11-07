use super::*;

#[test]
fn cleanup_does_not_replace_success_or_native_failure() {
    for closed in [Ok(()), Err(AgentSessionError::Failed)] {
        assert_eq!(finish(Ok(42), closed), Ok(42));
        for error in [
            AgentSessionError::Rejected,
            AgentSessionError::Busy,
            AgentSessionError::Failed,
        ] {
            assert_eq!(finish::<()>(Err(error), closed), Err(error));
        }
    }
}
