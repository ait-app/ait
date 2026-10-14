use super::*;
use crate::device::test_support::{Fixture, declaration, response};
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn enrollment_retries_pending_and_temporary_outage_with_one_durable_request() {
    let machine = declaration();
    let fixture = Fixture::new(vec![
        (
            StatusCode::BAD_REQUEST,
            json!({"error":{"code":"authorization_pending"}}),
        ),
        (StatusCode::SERVICE_UNAVAILABLE, json!({})),
        (StatusCode::OK, response(&machine)),
    ])
    .await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    let request_id = Uuid::new_v4();
    let mut state = State {
        machine,
        credential: None,
        pending: Some(Pending::Enrollment {
            token: "fixture.enrollment".into(),
            request_id,
        }),
    };
    store.save(&state).unwrap();

    complete_login(
        &fixture.center,
        &store,
        &mut state,
        &CancellationToken::new(),
    )
    .await
    .unwrap();

    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 3);
    assert!(
        seen.calls
            .iter()
            .all(|call| call.1["request_id"] == request_id.to_string())
    );
    assert!(state.pending.is_none());
    assert!(store.load().unwrap().unwrap().credential.is_some());
}

#[tokio::test]
async fn refresh_or_absent_login_operations_fail_without_contacting_the_center() {
    let fixture = Fixture::new(vec![]).await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    for pending in [None, Some(Pending::Refresh(Uuid::new_v4()))] {
        let mut state = State {
            machine: declaration(),
            credential: None,
            pending,
        };
        assert_eq!(
            complete_login(
                &fixture.center,
                &store,
                &mut state,
                &CancellationToken::new()
            )
            .await,
            Err(Error::Protocol)
        );
    }
    assert!(fixture.seen.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn cancelled_web_approval_and_enrollment_slowdown_preserve_pending_state() {
    let fixture = Fixture::new(vec![(
        StatusCode::BAD_REQUEST,
        json!({"error":{"code":"slow_down"}}),
    )])
    .await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    let mut state = State {
        machine: declaration(),
        credential: None,
        pending: Some(Pending::Web {
            device_code: "fixture-code".into(),
            user_code: "ABCD-1234-EF56".into(),
            request_id: Uuid::new_v4(),
            expires_at: Utc::now() + chrono::Duration::minutes(10),
            interval: 5,
        }),
    };
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        complete_login(&fixture.center, &store, &mut state, &cancel).await,
        Err(Error::Denied)
    );
    assert!(fixture.seen.lock().unwrap().calls.is_empty());
    assert!(matches!(state.pending, Some(Pending::Web { .. })));

    state.pending = Some(Pending::Enrollment {
        token: "fixture.token".into(),
        request_id: Uuid::new_v4(),
    });
    store.save(&state).unwrap();
    assert_eq!(
        complete_login(
            &fixture.center,
            &store,
            &mut state,
            &CancellationToken::new()
        )
        .await,
        Err(Error::Protocol)
    );
    assert!(store.load().unwrap().unwrap().credential.is_none());
    assert!(matches!(state.pending, Some(Pending::Enrollment { .. })));
}

#[tokio::test]
async fn logout_without_saved_credentials_clears_locally_without_contacting_center() {
    let data = tempfile::tempdir().unwrap();
    logout(data.path()).await.unwrap();
    assert!(Store::open(data.path()).unwrap().load().unwrap().is_none());
    assert!(InstanceLease::acquire(data.path()).is_ok());
}
