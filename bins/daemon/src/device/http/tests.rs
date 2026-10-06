use super::*;
use crate::device::test_support::{Fixture, binding, declaration, response};
use axum::http::StatusCode;

#[tokio::test]
async fn exchanges_poll_enrollment_and_refresh_without_user_api_access() {
    let machine = declaration();
    let wire = response(&machine);
    let fixture = Fixture::new(vec![
        (StatusCode::OK, json!({"device_code":"b".repeat(64),"user_code":"ABCD-1234-EF56","verification_uri":VERIFICATION_URI,"expires_in":600,"interval":5})),
        (StatusCode::OK, wire.clone()), (StatusCode::OK, wire.clone()), (StatusCode::OK, wire.clone()), (StatusCode::NO_CONTENT, Value::Null),
    ]).await;
    let request_id = Uuid::new_v4();
    let auth = fixture.center.authorize(&machine).await.unwrap();
    assert_eq!(auth.user_code, "ABCD-1234-EF56");
    assert!(!format!("{auth:?}").contains(&"b".repeat(64)));
    let tokens = fixture
        .center
        .poll(&auth.device_code, request_id)
        .await
        .unwrap();
    fixture
        .center
        .enroll(&"enrollment.jwt.secret".into(), request_id, &machine)
        .await
        .unwrap();
    fixture
        .center
        .refresh(&tokens.credential.refresh_token, request_id)
        .await
        .unwrap();
    fixture
        .center
        .revoke(&tokens.credential.refresh_token)
        .await
        .unwrap();
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(seen.calls.len(), 5);
    assert_eq!(seen.calls[0].1["client_id"], "ait-linux-daemon");
    assert_eq!(seen.calls[1].1["request_id"], request_id.to_string());
    assert!(seen.calls.iter().all(|(_, _, auth)| auth.is_none()));
}

#[tokio::test]
async fn validates_runtime_binding_and_uses_latest_access_token_for_lease_and_ticket() {
    let machine = declaration();
    let wire = response(&machine);
    let tokens = decode_tokens(wire.clone()).unwrap();
    let session = Uuid::new_v4();
    let fixture = Fixture::new(vec![
        (StatusCode::OK, json!({"node_id":wire["node_id"],"host_id":wire["host_id"],"server_id":machine.server_id,"node_session_id":session,
            "lease_duration_seconds":60,"renew_after_seconds":20,"control_required":true})),
        (StatusCode::OK, json!({"lease_until":Utc::now()+chrono::Duration::seconds(60),"renew_after_seconds":20})),
        (StatusCode::OK, json!({"control_ticket":"c".repeat(64),"expires_in_seconds":30})),
        (StatusCode::NO_CONTENT, Value::Null),
    ]).await;
    let instance = Uuid::new_v4();
    assert_eq!(
        fixture
            .center
            .register(&tokens, &machine, instance, Uuid::new_v4())
            .await
            .unwrap()
            .id,
        session
    );
    fixture.center.renew(&tokens, session).await.unwrap();
    let ticket = fixture.center.ticket(&tokens, session).await.unwrap();
    assert_eq!(ticket.expose_secret(), "c".repeat(64));
    fixture.center.close(&tokens, session).await.unwrap();
    let seen = fixture.seen.lock().unwrap();
    assert_eq!(
        seen.calls[0].1["installation_id"],
        machine.server_id.to_string()
    );
    assert_eq!(
        seen.calls[0].1["runtime"]["instance_id"],
        instance.to_string()
    );
    assert!(
        seen.calls
            .iter()
            .all(|(_, _, auth)| auth.as_deref() == Some("Bearer jwt.device.secret"))
    );
    assert_eq!(tokens.credential.binding, binding(&wire));
}

#[tokio::test]
async fn errors_are_sanitized_and_distinguish_polling_session_and_revocation() {
    let cases = [
        (400, "authorization_pending", Error::Pending),
        (400, "slow_down", Error::SlowDown),
        (400, "access_denied", Error::Denied),
        (400, "expired_token", Error::Denied),
        (400, "refresh_reuse", Error::Unauthorized),
        (401, "node_session_expired", Error::SessionExpired),
        (401, "unauthorized", Error::Unauthorized),
        (403, "forbidden", Error::Unauthorized),
        (409, "conflict", Error::Conflict),
        (410, "gone", Error::SessionExpired),
        (404, "not_found", Error::SessionExpired),
        (429, "device_rate_limited", Error::Unavailable),
        (503, "device_auth_disabled", Error::Unavailable),
        (400, "unexpected", Error::Protocol),
    ];
    let fixture = Fixture::new(
        cases
            .iter()
            .map(|(status, code, _)| {
                (
                    StatusCode::from_u16(*status).unwrap(),
                    json!({"error":{"code":code,"message":"sensitive center detail"}}),
                )
            })
            .collect(),
    )
    .await;
    for (_, _, expected) in cases {
        let error = fixture
            .center
            .poll(&"private-code".into(), Uuid::new_v4())
            .await
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("sensitive"));
    }
}

#[tokio::test]
async fn rejects_untrusted_verification_uri_bindings_tickets_and_redirects() {
    let machine = declaration();
    let tokens = decode_tokens(response(&machine)).unwrap();
    let fixture = Fixture::new(vec![
        (StatusCode::OK, json!({"device_code":"b".repeat(64),"user_code":"ABCD-1234-EF56","verification_uri":"https://evil.test","expires_in":600,"interval":5})),
        (StatusCode::OK, json!({"node_id":Uuid::new_v4(),"host_id":Uuid::new_v4(),"server_id":machine.server_id,"control_required":true})),
        (StatusCode::OK, json!({"control_ticket":"bad"})),
        (StatusCode::OK, json!({"lease_until":Utc::now()-chrono::Duration::seconds(1)})),
        (StatusCode::FOUND, json!({"secret":"never echoed"})),
        (StatusCode::OK, json!({"oversized":"x".repeat(65537)})),
    ]).await;
    assert_eq!(
        fixture.center.authorize(&machine).await.unwrap_err(),
        Error::Protocol
    );
    assert_eq!(
        fixture
            .center
            .register(&tokens, &machine, Uuid::new_v4(), Uuid::new_v4())
            .await
            .unwrap_err(),
        Error::Protocol
    );
    assert_eq!(
        fixture
            .center
            .ticket(&tokens, Uuid::new_v4())
            .await
            .unwrap_err(),
        Error::Protocol
    );
    assert_eq!(
        fixture
            .center
            .renew(&tokens, Uuid::new_v4())
            .await
            .unwrap_err(),
        Error::Protocol
    );
    assert_eq!(
        fixture
            .center
            .poll(&"code".into(), Uuid::new_v4())
            .await
            .unwrap_err(),
        Error::Protocol
    );
    assert_eq!(
        fixture
            .center
            .poll(&"code".into(), Uuid::new_v4())
            .await
            .unwrap_err(),
        Error::Protocol
    );
    assert_eq!(fixture.seen.lock().unwrap().calls.len(), 6);
}

#[test]
fn original_expiry_is_preserved_for_recovery_receipts_and_invalid_tokens_rejected() {
    let original = response(&declaration());
    let mut recovered = original.clone();
    recovered["access_expires_at"] = json!(Utc::now() - chrono::Duration::seconds(1));
    let tokens = decode_tokens(recovered.clone()).unwrap();
    assert!(tokens.access_expires_at < Utc::now());
    for (key, value) in [
        ("token_type", json!("wrong")),
        ("access_token", json!("bad token")),
        ("refresh_token", json!("invalid")),
        ("refresh_expires_at", json!(Utc::now())),
        (
            "access_expires_at",
            json!(Utc::now() + chrono::Duration::days(1)),
        ),
        ("node_id", json!(Uuid::nil())),
    ] {
        let mut invalid = original.clone();
        invalid[key] = value;
        assert!(decode_tokens(invalid).is_err());
    }
    assert_eq!(HttpCenter::new().unwrap().base, CENTER);
}
