use super::*;
use crate::local::opencode::{
    Driver,
    tests::{fixture::Fixture, invocation},
};

#[tokio::test]
async fn permission_validation_distinguishes_invalid_effects_from_native_contention() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let mut connection = Driver::new(fixture.binary.clone())
            .open(invocation(fixture.cwd.clone()))
            .await
            .unwrap();
        fixture.state.lock().unwrap().busy = true;
        for (effect, expected) in [
            ("auto", Fault::AgentCapabilityUnsupported),
            ("ask", Fault::SessionBusy),
        ] {
            assert_eq!(
                connection
                    .runtime
                    .api
                    .set_permission(&connection.prepared.id, effect)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
        }
        {
            let state = fixture.state.lock().unwrap();
            assert_eq!(state.permission_updates, 0);
            assert_eq!(state.submissions, 0);
        }
        connection.close().await;
    }
}
