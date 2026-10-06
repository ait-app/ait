use super::*;
use clap::Parser;

#[test]
fn fixed_center_and_login_modes_are_unambiguous_and_redacted() {
    let cli = crate::config::Cli::try_parse_from([
        "daemon",
        "login",
        "--token",
        "abc.def.ghi",
        "--name",
        "build-linux",
    ])
    .unwrap();
    assert!(!format!("{cli:?}").contains("abc.def.ghi"));
    assert!(matches!(
        cli.command,
        Some(crate::config::Command::Login(_))
    ));
    for args in [
        vec!["daemon", "login", "--token", "a", "--token-stdin"],
        vec!["daemon", "login", "--center", "https://evil.test"],
        vec![
            "daemon",
            "run",
            "--headless",
            "--center",
            "https://evil.test",
        ],
        vec!["daemon", "service", "install", "--user"],
    ] {
        assert!(crate::config::Cli::try_parse_from(args).is_err());
    }
    assert_eq!(host_link::CENTER, "https://dash.ait-app.com:8443/api");
    assert!(read_token(Some("".into()), false).is_err());
    assert!(read_token(Some("invalid token".into()), false).is_err());
    assert!(read_token(Some("x".repeat(8193).into()), false).is_err());
    assert!(read_token(None, false).unwrap().is_none());
}

#[tokio::test]
async fn enrollment_response_is_durable_and_recovery_keeps_the_same_request() {
    use crate::device::test_support::{Fixture, declaration, response};
    let machine = declaration();
    let wire = response(&machine);
    let fixture = Fixture::new(vec![(axum::http::StatusCode::OK, wire)]).await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    let id = Uuid::new_v4();
    let mut state = State {
        machine,
        credential: None,
        pending: Some(Pending::Enrollment {
            token: "single.use.jwt".into(),
            request_id: id,
        }),
    };
    store.save(&state).unwrap();
    let mut restored = store.load().unwrap().unwrap();
    complete_login(
        &fixture.center,
        &store,
        &mut restored,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(store.load().unwrap().unwrap().pending.is_none());
    assert!(store.load().unwrap().unwrap().credential.is_some());
    assert_eq!(
        fixture.seen.lock().unwrap().calls[0].1["request_id"],
        id.to_string()
    );
    state.machine.server_id = Uuid::new_v4();
    assert!(read_token(Some("bad token".into()), false).is_err());
}

#[tokio::test]
async fn enrollment_mismatch_and_expired_web_approval_never_publish_credentials() {
    use crate::device::test_support::{Fixture, declaration, response};
    let machine = declaration();
    let fixture = Fixture::new(vec![(axum::http::StatusCode::OK, response(&declaration()))]).await;
    let data = tempfile::tempdir().unwrap();
    let store = Store::open(data.path()).unwrap();
    let mut state = State {
        machine,
        credential: None,
        pending: Some(Pending::Enrollment {
            token: "single.use.jwt".into(),
            request_id: Uuid::new_v4(),
        }),
    };
    store.save(&state).unwrap();
    assert_eq!(
        complete_login(
            &fixture.center,
            &store,
            &mut state,
            &CancellationToken::new()
        )
        .await
        .unwrap_err(),
        Error::Protocol
    );
    assert!(store.load().unwrap().unwrap().credential.is_none());
    state.pending = Some(Pending::Web {
        device_code: "secret-code".into(),
        user_code: "ABCD-1234-EF56".into(),
        request_id: Uuid::new_v4(),
        expires_at: Utc::now(),
        interval: 5,
    });
    assert_eq!(
        complete_login(
            &fixture.center,
            &store,
            &mut state,
            &CancellationToken::new()
        )
        .await
        .unwrap_err(),
        Error::Denied
    );
    assert_eq!(fixture.seen.lock().unwrap().calls.len(), 1);
}

fn machine_for(data: &Path) -> Machine {
    let lease = InstanceLease::acquire(data).unwrap();
    Machine {
        server_id: lease.server_id,
        display_name: "build-linux".into(),
        platform: "linux".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    }
}

#[tokio::test]
async fn complete_token_login_and_online_logout_use_same_installation_and_clear_state() {
    use crate::device::test_support::{Fixture, response};
    let data = tempfile::tempdir().unwrap();
    let machine = machine_for(data.path());
    let fixture = Fixture::new(vec![
        (axum::http::StatusCode::OK, response(&machine)),
        (axum::http::StatusCode::NO_CONTENT, serde_json::Value::Null),
    ])
    .await;
    login_with_center(
        data.path(),
        Login {
            name: Some(machine.display_name),
            token: Some("single.use.jwt".into()),
            token_stdin: false,
        },
        fixture.center.clone(),
    )
    .await
    .unwrap();
    let store = Store::open(data.path()).unwrap();
    let state = store.load().unwrap().unwrap();
    assert!(state.pending.is_none());
    assert_eq!(state.machine.server_id, machine.server_id);
    drop(store);
    logout_with_center(data.path(), fixture.center.clone())
        .await
        .unwrap();
    assert!(Store::open(data.path()).unwrap().load().unwrap().is_none());
    assert_eq!(
        fixture.seen.lock().unwrap().calls[1].0,
        "/v1/auth/device/revoke"
    );
}

#[tokio::test]
async fn web_login_polls_then_preserves_slowdown_and_recovers_after_interruption() {
    use crate::device::test_support::{Fixture, response};
    let data = tempfile::tempdir().unwrap();
    let machine = machine_for(data.path());
    let fixture = Fixture::new(vec![
        (axum::http::StatusCode::OK, serde_json::json!({"device_code":"a".repeat(64),"user_code":"ABCD-1234-EF56","verification_uri":VERIFICATION_URI,"expires_in":600,"interval":5})),
        (axum::http::StatusCode::BAD_REQUEST, serde_json::json!({"error":{"code":"slow_down"}})),
        (axum::http::StatusCode::OK, response(&machine)),
    ]).await;
    login_with_center(
        data.path(),
        Login {
            name: Some(machine.display_name),
            token: None,
            token_stdin: false,
        },
        fixture.center.clone(),
    )
    .await
    .unwrap();
    {
        let seen = fixture.seen.lock().unwrap();
        assert_eq!(seen.calls.len(), 3);
        assert_eq!(seen.calls[1].1["request_id"], seen.calls[2].1["request_id"]);
    }
    let store = Store::open(data.path()).unwrap();
    let mut state = store.load().unwrap().unwrap();
    state.pending = Some(Pending::Enrollment {
        token: "recovery.jwt.secret".into(),
        request_id: Uuid::new_v4(),
    });
    store.save(&state).unwrap();
    drop(store);
    fixture
        .seen
        .lock()
        .unwrap()
        .responses
        .push_back((axum::http::StatusCode::OK, response(&state.machine)));
    login_with_center(
        data.path(),
        Login {
            name: None,
            token: None,
            token_stdin: false,
        },
        fixture.center.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        fixture.seen.lock().unwrap().calls.last().unwrap().0,
        "/v1/auth/device/enroll"
    );
}

#[tokio::test]
async fn rejects_competing_or_invalid_logins_and_clears_local_state_when_center_is_offline() {
    use crate::device::test_support::Fixture;
    let data = tempfile::tempdir().unwrap();
    let machine = machine_for(data.path());
    let fixture = Fixture::new(vec![]).await;
    let store = Store::open(data.path()).unwrap();
    let state = State {
        machine,
        credential: None,
        pending: Some(Pending::Enrollment {
            token: "recover.jwt.secret".into(),
            request_id: Uuid::new_v4(),
        }),
    };
    store.save(&state).unwrap();
    drop(store);
    for options in [
        Login {
            name: Some(String::new()),
            token: None,
            token_stdin: false,
        },
        Login {
            name: None,
            token: Some("replacement.jwt.secret".into()),
            token_stdin: false,
        },
    ] {
        assert!(
            login_with_center(data.path(), options, fixture.center.clone())
                .await
                .is_err()
        );
    }
    assert!(fixture.seen.lock().unwrap().calls.is_empty());
    logout_with_center(data.path(), fixture.center.clone())
        .await
        .unwrap();
    assert!(!data.path().join("device/.env").exists());
    status(data.path(), false).unwrap();
}
