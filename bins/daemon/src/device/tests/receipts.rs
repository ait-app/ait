use super::*;
use crate::device::test_support::{Fixture, declaration, response};

#[tokio::test]
async fn recovers_consumed_device_code_after_approval_deadline() {
    let machine = declaration();
    // Center consumed this code shortly before its deadline; its issuance receipt is still valid.
    let fixture = Fixture::new(vec![(axum::http::StatusCode::OK, response(&machine))]).await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    let id = Uuid::new_v4();
    let mut state = State {
        disconnected: false,
        authorization_id: Some(id),
        machine,
        credential: None,
        pending: Some(Pending::Web {
            device_code: "c".repeat(64).into(),
            user_code: "ABCD-0123-ABCD".into(),
            request_id: id,
            expires_at: Utc::now() - chrono::Duration::seconds(1),
            interval: 5,
        }),
    };
    store.save(&state).unwrap();
    let result = complete_login(
        &fixture.center,
        &store,
        &mut state,
        &CancellationToken::new(),
    )
    .await;
    assert_eq!(
        result,
        Ok(()),
        "the center's valid issuance receipt must be recoverable with the original request id"
    );
    assert_eq!(fixture.seen.lock().unwrap().calls.len(), 1);
}
