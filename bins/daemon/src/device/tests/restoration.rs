use super::*;
use crate::device::test_support::{Fixture, binding, response};
use axum::http::StatusCode;
use host_link::Credential;
use secrecy::ExposeSecret;
use serde_json::{Value, json};

fn saved_machine(data: &Path) -> (State, Value) {
    let machine = machine_for(data);
    let wire = response(&machine);
    let state = State {
        machine,
        credential: Some(Credential {
            refresh_token: "saved-refresh-secret".into(),
            refresh_expires_at: Utc::now() + chrono::Duration::days(30),
            binding: binding(&wire),
        }),
        pending: None,
    };
    Store::open(data).unwrap().save(&state).unwrap();
    (state, wire)
}

fn options(force: bool) -> Login {
    Login {
        name: None,
        token: None,
        token_stdin: false,
        force,
    }
}

fn approval() -> Value {
    json!({"device_code":"a".repeat(64),"user_code":"ABCD-1234-EF56",
        "verification_uri":VERIFICATION_URI,"expires_in":600,"interval":5})
}

#[tokio::test]
async fn login_restores_saved_credentials_without_starting_web_authorization() {
    let data = tempfile::tempdir().unwrap();
    let (saved, wire) = saved_machine(data.path());
    let fixture = Fixture::new(vec![(StatusCode::OK, wire.clone())]).await;
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    let restored = Store::open(data.path()).unwrap().load().unwrap().unwrap();
    assert!(restored.pending.is_none());
    assert_eq!(
        restored.credential.as_ref().unwrap().binding,
        saved.credential.unwrap().binding
    );
    assert_eq!(
        restored.credential.unwrap().refresh_token.expose_secret(),
        wire["refresh_token"].as_str().unwrap()
    );
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 1);
    assert_eq!(seen.calls[0].0, "/v1/auth/device/refresh");
    assert_eq!(seen.calls[0].1["refresh_token"], "saved-refresh-secret");
}

#[tokio::test]
async fn restart_failure_can_reauthorize_without_logout_or_changing_machine_identity() {
    let data = tempfile::tempdir().unwrap();
    let (mut saved, wire) = saved_machine(data.path());
    let request_id = Uuid::new_v4();
    saved.pending = Some(Pending::Refresh(request_id));
    Store::open(data.path()).unwrap().save(&saved).unwrap();
    let fixture = Fixture::new(vec![
        (
            StatusCode::UNAUTHORIZED,
            json!({"error":{"code":"invalid_token"}}),
        ),
        (StatusCode::OK, approval()),
        (StatusCode::OK, wire),
    ])
    .await;
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    let restored = Store::open(data.path()).unwrap().load().unwrap().unwrap();
    assert_eq!(restored.machine.server_id, saved.machine.server_id);
    assert!(restored.pending.is_none());
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(
        seen.calls
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        [
            "/v1/auth/device/refresh",
            "/v1/auth/device/authorize",
            "/v1/auth/device/token"
        ]
    );
    assert_eq!(seen.calls[0].1["request_id"], request_id.to_string());
}

#[tokio::test]
async fn transient_and_protocol_failures_keep_credentials_and_do_not_trigger_login() {
    for (status, body) in [
        (StatusCode::SERVICE_UNAVAILABLE, json!({})),
        (
            StatusCode::NOT_FOUND,
            json!({"error":{"code":"missing_endpoint"}}),
        ),
    ] {
        let data = tempfile::tempdir().unwrap();
        let (saved, _) = saved_machine(data.path());
        let fixture = Fixture::new(vec![(status, body)]).await;
        assert!(
            login_with_center(data.path(), options(false), fixture.center.clone())
                .await
                .is_err()
        );
        let restored = Store::open(data.path()).unwrap().load().unwrap().unwrap();
        assert!(matches!(restored.pending, Some(Pending::Refresh(_))));
        assert_eq!(
            restored.credential.unwrap().refresh_token.expose_secret(),
            saved.credential.unwrap().refresh_token.expose_secret()
        );
        assert_eq!(fixture.seen.lock().unwrap().calls.len(), 1);
    }
}

#[tokio::test]
async fn interrupted_refresh_replays_the_same_request_on_next_login() {
    let data = tempfile::tempdir().unwrap();
    let (_, wire) = saved_machine(data.path());
    let fixture = Fixture::new(vec![
        (StatusCode::SERVICE_UNAVAILABLE, json!({})),
        (StatusCode::OK, wire),
    ])
    .await;
    assert!(
        login_with_center(data.path(), options(false), fixture.center.clone())
            .await
            .is_err()
    );
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 2);
    assert_eq!(seen.calls[0].1, seen.calls[1].1);
}

#[tokio::test]
async fn force_login_explicitly_replaces_pending_authorization() {
    let data = tempfile::tempdir().unwrap();
    let (mut saved, wire) = saved_machine(data.path());
    saved.pending = Some(Pending::Refresh(Uuid::new_v4()));
    Store::open(data.path()).unwrap().save(&saved).unwrap();
    let fixture = Fixture::new(vec![(StatusCode::OK, approval()), (StatusCode::OK, wire)]).await;
    login_with_center(data.path(), options(true), fixture.center.clone())
        .await
        .unwrap();
    assert_eq!(
        fixture.seen.lock().unwrap().calls[0].0,
        "/v1/auth/device/authorize"
    );
    assert!(
        Store::open(data.path())
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
            .pending
            .is_none()
    );
}

#[tokio::test]
async fn expired_credentials_start_new_authorization_without_refreshing() {
    let data = tempfile::tempdir().unwrap();
    let (mut saved, wire) = saved_machine(data.path());
    saved.credential.as_mut().unwrap().refresh_expires_at =
        Utc::now() - chrono::Duration::seconds(1);
    Store::open(data.path()).unwrap().save(&saved).unwrap();
    let fixture = Fixture::new(vec![(StatusCode::OK, approval()), (StatusCode::OK, wire)]).await;
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    assert_eq!(fixture.seen.lock().unwrap().calls.len(), 2);
    assert_eq!(
        fixture.seen.lock().unwrap().calls[0].0,
        "/v1/auth/device/authorize"
    );
}

#[tokio::test]
async fn expired_pending_receipt_is_saved_then_refreshed_without_web_login() {
    let data = tempfile::tempdir().unwrap();
    let (mut saved, wire) = saved_machine(data.path());
    let request_id = Uuid::new_v4();
    saved.pending = Some(Pending::Refresh(request_id));
    saved.credential.as_mut().unwrap().refresh_expires_at =
        Utc::now() - chrono::Duration::seconds(1);
    Store::open(data.path()).unwrap().save(&saved).unwrap();
    let mut receipt = wire.clone();
    receipt["access_expires_at"] = json!(Utc::now() - chrono::Duration::seconds(1));
    let fixture = Fixture::new(vec![
        (StatusCode::OK, receipt),
        (StatusCode::OK, wire.clone()),
    ])
    .await;
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 2);
    assert!(
        seen.calls
            .iter()
            .all(|call| call.0 == "/v1/auth/device/refresh")
    );
    assert_eq!(seen.calls[0].1["request_id"], request_id.to_string());
    assert_ne!(seen.calls[1].1["request_id"], request_id.to_string());
    assert_eq!(seen.calls[1].1["refresh_token"], wire["refresh_token"]);
}

#[tokio::test]
async fn expired_browser_approval_is_replaced_without_requiring_logout() {
    let data = tempfile::tempdir().unwrap();
    let (mut saved, wire) = saved_machine(data.path());
    saved.pending = Some(Pending::Web {
        device_code: "expired-device-code".into(),
        user_code: "ABCD-1234-EF56".into(),
        request_id: Uuid::new_v4(),
        expires_at: Utc::now() - chrono::Duration::seconds(1),
        interval: 5,
    });
    Store::open(data.path()).unwrap().save(&saved).unwrap();
    let fixture = Fixture::new(vec![(StatusCode::OK, approval()), (StatusCode::OK, wire)]).await;
    login_with_center(data.path(), options(false), fixture.center.clone())
        .await
        .unwrap();
    assert_eq!(
        fixture.seen.lock().unwrap().calls[0].0,
        "/v1/auth/device/authorize"
    );
}

#[tokio::test]
async fn saved_name_survives_hostname_changes_and_explicit_name_overrides_it() {
    for explicit in [None, Some("My Mac")] {
        let data = tempfile::tempdir().unwrap();
        let (saved, wire) = saved_machine(data.path());
        let mut fixture = Fixture::new(vec![(StatusCode::OK, wire)]).await;
        fixture.center.hostname = Some("changed-hostname".into());
        let mut login = options(false);
        login.name = explicit.map(str::to_owned);
        login_with_center(data.path(), login, fixture.center.clone())
            .await
            .unwrap();
        let restored = Store::open(data.path()).unwrap().load().unwrap().unwrap();
        assert_eq!(
            restored.machine.display_name,
            explicit.unwrap_or(&saved.machine.display_name)
        );
        assert_eq!(restored.machine.platform, PLATFORM);
    }
}

#[tokio::test]
async fn first_authorization_uses_hostname_or_portable_fallback() {
    for hostname in [Some("mac-mini"), None] {
        let data = tempfile::tempdir().unwrap();
        let machine = machine_for(data.path());
        let mut fixture = Fixture::new(vec![
            (StatusCode::OK, approval()),
            (StatusCode::OK, response(&machine)),
        ])
        .await;
        fixture.center.hostname = hostname.map(str::to_owned);
        login_with_center(data.path(), options(false), fixture.center.clone())
            .await
            .unwrap();
        let restored = Store::open(data.path()).unwrap().load().unwrap().unwrap();
        assert_eq!(
            restored.machine.display_name,
            hostname.unwrap_or("ait-daemon")
        );
        assert_eq!(restored.machine.platform, PLATFORM);
    }
}
