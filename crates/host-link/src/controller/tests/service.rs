use super::*;
use crate::control::{Authorization, AuthorizationMode};
use crate::service::Service;

fn service(authorized: bool) -> (Service<Mock, Mock, Mock>, Mock) {
    let (controller, mock) = fixture();
    let mut state = controller.state;
    if !authorized {
        state.credential = None;
    }
    (
        Service::new(
            mock.clone(),
            mock.clone(),
            mock.clone(),
            state,
            Uuid::new_v4(),
        ),
        mock,
    )
}

fn credential(mock: &Mock, request_id: Uuid) -> Authorization {
    Authorization::Credential {
        request_id,
        refresh_token: "external-refresh-secret".into(),
        refresh_expires_at: Utc::now() + chrono::Duration::days(30),
        binding: mock.1.clone(),
    }
}

#[tokio::test(start_paused = true)]
async fn starts_without_credentials_and_persists_handoff_before_consumption() {
    let (service, mock) = service(false);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    assert_eq!(mock.0.lock().unwrap().reports, ["unauthorized"]);
    assert!(mock.0.lock().unwrap().refreshes.is_empty());
    let id = Uuid::new_v4();
    control
        .authorize(credential(&mock, id), AuthorizationMode::Explicit)
        .await
        .unwrap();
    tick().await;
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.saved[0].authorization_id, Some(id));
        assert!(seen.saved[0].pending.is_none());
        assert_eq!(seen.refreshes[0].0, "external-refresh-secret");
        assert!(seen.active && seen.hub.active);
    }
    // A lost HTTP response must never reinstall an already consumed refresh secret.
    control
        .authorize(credential(&mock, id), AuthorizationMode::Explicit)
        .await
        .unwrap();
    tick().await;
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 1);
    assert_eq!(mock.0.lock().unwrap().registrations.len(), 1);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn disconnect_clears_shared_authorization_and_stops_both_links_and_renewal() {
    let (service, mock) = service(true);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    control.disconnect().await.unwrap();
    let renewals = mock.0.lock().unwrap().renewals;
    for _ in 0..70 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert!(!seen.active && !seen.hub.active);
        assert_eq!(seen.renewals, renewals);
        assert_eq!(seen.closes, 1);
        assert_eq!(seen.revocations, ["new-refresh-secret"]);
        assert_eq!(seen.relay_grants, seen.hub_grants);
        let saved = seen.saved.last().unwrap();
        assert!(saved.disconnected);
        assert!(
            saved.credential.is_none()
                && saved.pending.is_none()
                && saved.authorization_id.is_none()
        );
        assert_eq!(seen.reports.last(), Some(&"disconnected"));
    }
    control.disconnect().await.unwrap(); // Idempotent and does not try revocation with a discarded secret.
    assert_eq!(mock.0.lock().unwrap().revocations.len(), 1);
    cancel.cancel();
    task.await.unwrap();
    let saved = mock.0.lock().unwrap().saved.last().unwrap().clone();
    let restarted = Service::new(
        mock.clone(),
        mock.clone(),
        mock.clone(),
        saved,
        Uuid::new_v4(),
    );
    let cancel = CancellationToken::new();
    let task = tokio::spawn(restarted.run(cancel.clone()));
    tick().await;
    assert_eq!(mock.0.lock().unwrap().registrations.len(), 1);
    assert_eq!(mock.0.lock().unwrap().reports.last(), Some(&"disconnected"));
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn delayed_tickets_cannot_restart_either_link_after_disconnect() {
    let (service, mock) = service(true);
    {
        let mut seen = mock.0.lock().unwrap();
        seen.hub.ticket_delay = Duration::from_secs(3);
        seen.ticket_delay = Duration::from_secs(3);
    }
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    control.disconnect().await.unwrap();
    for _ in 0..5 {
        tick().await;
    }
    assert_eq!(mock.0.lock().unwrap().hub.starts, 0);
    assert_eq!(mock.0.lock().unwrap().starts, 0);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn reauthorization_after_terminal_failure_does_not_require_process_restart() {
    let (service, mock) = service(true);
    mock.0.lock().unwrap().refresh_error = Some(Error::Unauthorized);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    assert!(!task.is_finished());
    assert_eq!(
        mock.0.lock().unwrap().reports.last(),
        Some(&"reauthorization_required")
    );
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    tick().await;
    assert!(mock.0.lock().unwrap().hub.active);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn failed_install_does_not_activate_uncommitted_credentials() {
    let (service, mock) = service(false);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    mock.0.lock().unwrap().save_fail_at = Some(0);
    assert_eq!(
        control
            .authorize(
                credential(&mock, Uuid::new_v4()),
                AuthorizationMode::Explicit
            )
            .await,
        Err(Error::Storage)
    );
    assert!(mock.0.lock().unwrap().refreshes.is_empty());
    mock.0.lock().unwrap().save_fail_at = None;
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    tick().await;
    assert!(mock.0.lock().unwrap().hub.active);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn foreign_authorization_does_not_interrupt_existing_publication() {
    let (service, mock) = service(true);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    let mut foreign = mock.clone();
    foreign.1.server_id = Uuid::new_v4();
    let authorization = credential(&foreign, Uuid::new_v4());
    assert!(!format!("{authorization:?}").contains("external-refresh-secret"));
    assert_eq!(
        control
            .authorize(authorization, AuthorizationMode::Explicit)
            .await,
        Err(Error::Protocol)
    );
    assert!(mock.0.lock().unwrap().hub.active);
    assert_eq!(mock.0.lock().unwrap().closes, 0);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn enrollment_outage_past_one_attempt_window_keeps_retrying_the_durable_request() {
    let (service, mock) = service(false);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    mock.0.lock().unwrap().enrollment_error = Some(Error::Unavailable);
    let id = Uuid::new_v4();
    control
        .authorize(
            Authorization::Enrollment {
                token: "one-use-enrollment".into(),
                request_id: id,
            },
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    for _ in 0..610 {
        tick().await;
    }
    assert!(!task.is_finished());
    {
        let seen = mock.0.lock().unwrap();
        assert!((10..=20).contains(&seen.enrollment_requests.len()));
        assert!(
            seen.enrollment_requests
                .iter()
                .all(|request| *request == id)
        );
        let waits: Vec<_> = seen
            .enrollment_times
            .windows(2)
            .map(|pair| pair[1].duration_since(pair[0]).as_secs())
            .collect();
        assert_eq!(&waits[..5], &[5, 10, 20, 40, 60]);
        assert!(waits[5..].iter().all(|seconds| *seconds == 60));
    }
    mock.0.lock().unwrap().enrollment_error = None;
    for _ in 0..61 {
        tick().await;
    }
    assert!(mock.0.lock().unwrap().hub.active);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn fresh_login_after_disconnect_starts_both_links_with_the_same_authority() {
    let (service, mock) = service(true);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    control.disconnect().await.unwrap();
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    tick().await;
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.registrations.len(), 2);
        assert_eq!(seen.closes, 1);
        assert!(seen.active && seen.hub.active);
        assert_eq!(seen.hub.starts, 2);
        assert_eq!(seen.relay_grants, seen.hub_grants);
        assert!(!seen.saved.last().unwrap().disconnected);
    }
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn disconnect_drops_pending_enrollment_and_does_not_retry_issuance() {
    let (service, mock) = service(false);
    mock.0.lock().unwrap().enrollment_error = Some(Error::Unavailable);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    control
        .authorize(
            Authorization::Enrollment {
                token: "pending-enrollment".into(),
                request_id: Uuid::new_v4(),
            },
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    tick().await;
    control.disconnect().await.unwrap();
    let requests = mock.0.lock().unwrap().enrollment_requests.len();
    for _ in 0..70 {
        tick().await;
    }
    let saved = mock.0.lock().unwrap().saved.last().unwrap().clone();
    assert!(
        saved.pending.is_none() && saved.credential.is_none() && saved.authorization_id.is_none()
    );
    assert_eq!(mock.0.lock().unwrap().enrollment_requests.len(), requests);
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn revocation_outage_never_prevents_bounded_local_disconnect() {
    for delay in [Duration::ZERO, Duration::from_secs(600)] {
        let (service, mock) = service(true);
        {
            let mut seen = mock.0.lock().unwrap();
            seen.revoke_delay = delay;
            seen.revoke_error = Some(Error::Unavailable);
        }
        let control = service.handle();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(service.run(cancel.clone()));
        tick().await;
        tokio::time::timeout(Duration::from_secs(6), control.disconnect())
            .await
            .unwrap()
            .unwrap();
        assert!(
            mock.0
                .lock()
                .unwrap()
                .saved
                .last()
                .unwrap()
                .credential
                .is_none()
        );
        cancel.cancel();
        task.await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn failed_durable_disconnect_reports_failure_but_never_resumes_in_memory() {
    let (service, mock) = service(true);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    {
        let mut seen = mock.0.lock().unwrap();
        seen.save_fail_at = Some(seen.saved.len());
    }
    assert_eq!(control.disconnect().await, Err(Error::Storage));
    tick().await;
    assert!(!mock.0.lock().unwrap().active);
    assert!(!mock.0.lock().unwrap().hub.active);
    assert_eq!(
        mock.0.lock().unwrap().reports.last(),
        Some(&"storage_error")
    );
    mock.0.lock().unwrap().save_fail_at = None;
    control.disconnect().await.unwrap();
    assert!(
        mock.0
            .lock()
            .unwrap()
            .saved
            .last()
            .unwrap()
            .credential
            .is_none()
    );
    cancel.cancel();
    task.await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn automatic_bootstrap_is_atomic_with_disconnect_and_preserves_existing_login() {
    let (service, mock) = service(false);
    let control = service.handle();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Initial,
        )
        .await
        .unwrap();
    tick().await;
    assert!(mock.0.lock().unwrap().active);
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Initial,
        )
        .await
        .unwrap();
    assert_eq!(mock.0.lock().unwrap().registrations.len(), 1);
    control.disconnect().await.unwrap();
    let saved = mock.0.lock().unwrap().saved.len();
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Initial,
        )
        .await
        .unwrap();
    tick().await;
    assert_eq!(mock.0.lock().unwrap().saved.len(), saved);
    assert!(mock.0.lock().unwrap().saved.last().unwrap().disconnected);
    assert!(!mock.0.lock().unwrap().active);
    control
        .authorize(
            credential(&mock, Uuid::new_v4()),
            AuthorizationMode::Explicit,
        )
        .await
        .unwrap();
    tick().await;
    assert!(mock.0.lock().unwrap().active);
    assert!(mock.0.lock().unwrap().hub.active);
    cancel.cancel();
    task.await.unwrap();
}
