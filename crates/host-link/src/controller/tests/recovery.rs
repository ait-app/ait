use super::*;
use crate::service::Service;

#[tokio::test(start_paused = true)]
async fn access_refusal_recovers_with_valid_refresh() {
    let (controller, mock) = fixture();
    let service = Service::new(
        mock.clone(),
        mock.clone(),
        mock.clone(),
        controller.state,
        Uuid::new_v4(),
    );
    let cancel = CancellationToken::new();
    let task = tokio::spawn(service.run(cancel.clone()));
    tick().await;
    mock.0.lock().unwrap().renew_error = Some(Error::AccessRejected);
    for _ in 0..10 {
        tick().await;
    }
    let observed = {
        let seen = mock.0.lock().unwrap();
        (seen.active, seen.hub.active, seen.refreshes.len())
    };
    cancel.cancel();
    task.await.unwrap();
    assert_eq!(
        observed,
        (true, true, 2),
        "an expired access JWT must probe the still-valid refresh credential before abandoning publication"
    );
}

#[tokio::test(start_paused = true)]
async fn persistent_access_refusal_probes_refresh_once_then_stops() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().ticket_error = Some(Error::AccessRejected);
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    // Let the paused runtime drive completion instead of assuming a fixed number of yields.
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("persistent ticket rejection must terminate within 10 seconds")
        .expect("controller task must not panic");
    assert_eq!(result, Err(Error::Unauthorized));
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.refreshes.len(), 2);
    assert_eq!(
        seen.renewals, 2,
        "queued ticket rejection must be handled before another lease renewal"
    );
    assert!(!seen.active && !seen.hub.active);
}

#[tokio::test(start_paused = true)]
async fn persistent_renewal_refusal_does_not_loop_on_refresh() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().renew_errors = [Error::AccessRejected; 2].into();
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("repeated 401 must terminate after one refresh probe within 10 seconds")
        .expect("controller task must not panic");
    assert_eq!(result, Err(Error::Unauthorized));
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn rejected_refresh_after_access_refusal_requires_login() {
    let (controller, mock) = fixture();
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    tick().await;
    {
        let mut seen = mock.0.lock().unwrap();
        seen.renew_error = Some(Error::AccessRejected);
        seen.refresh_error = Some(Error::Unauthorized);
    }
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("rejected refresh must require login within 5 seconds")
        .expect("controller task must not panic");
    assert_eq!(result, Err(Error::Unauthorized));
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
}

#[tokio::test(start_paused = true)]
async fn repeated_session_expiry_backs_off_registration() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().register_errors = Some(Error::SessionExpired);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..10 {
        tick().await;
    }
    let registrations = mock.0.lock().unwrap().registrations.len();
    assert!(
        registrations <= 5,
        "session expiry must not re-register without delay: {registrations} attempts in 10s"
    );
    mock.0.lock().unwrap().register_errors = None;
    tokio::time::advance(Duration::from_secs(30)).await;
    for _ in 0..3 {
        tick().await;
    }
    assert!(mock.0.lock().unwrap().active);
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn protocol_failures_retry_initial_refresh_with_the_same_request() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().refresh_error = Some(Error::Protocol);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..5 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.refreshes.len(), 2);
        assert_eq!(seen.refreshes[0], seen.refreshes[1]);
        assert!(seen.reports.contains(&"retrying"));
        assert!(seen.active && seen.hub.active);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn protocol_failures_retry_registration_and_renewal_without_reauthorization() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().register_error = Some(Error::Protocol);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..5 {
        tick().await;
    }
    {
        let mut seen = mock.0.lock().unwrap();
        assert!(seen.active && seen.hub.active);
        assert_eq!(seen.registrations.len(), 2);
        assert_eq!(seen.registrations[0], seen.registrations[1]);
        seen.renew_errors = [Error::Protocol; 3].into();
    }
    for _ in 0..10 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert!(seen.active && seen.hub.active);
        assert_eq!(seen.refreshes.len(), 1);
        assert_eq!(
            seen.reports
                .iter()
                .filter(|phase| **phase == "retrying")
                .count(),
            4
        );
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn accepted_successor_lease_can_recover_a_later_access_refusal() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    for expected_refreshes in [2, 3] {
        mock.0.lock().unwrap().renew_error = Some(Error::AccessRejected);
        for _ in 0..5 {
            tick().await;
        }
        let seen = mock.0.lock().unwrap();
        assert!(seen.active && seen.hub.active);
        assert_eq!(seen.refreshes.len(), expected_refreshes);
        assert!(!task.is_finished());
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn accepted_successor_ticket_can_recover_a_later_access_refusal() {
    for hub in [false, true] {
        let (controller, mock) = fixture();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(controller.run(cancel.clone()));
        wait_for_ticket_recovery(&mock, 1, hub).await;
        for expected_refreshes in [2, 3] {
            {
                let mut seen = mock.0.lock().unwrap();
                if hub {
                    seen.hub.active = false;
                    seen.hub.errors.push_back(Error::AccessRejected);
                } else {
                    seen.active = false;
                    seen.ticket_errors.push_back(Error::AccessRejected);
                }
            }
            wait_for_ticket_recovery(&mock, expected_refreshes, hub).await;
            assert!(!task.is_finished());
        }
        cancel.cancel();
        task.await.unwrap().unwrap();
    }
}

async fn wait_for_ticket_recovery(mock: &Mock, expected_refreshes: usize, hub: bool) {
    // Let the paused clock follow pending timers rather than treating a fixed number of
    // scheduler yields as completion of ticket issuance, refresh and both reconnects.
    let recovered = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            {
                let seen = mock.0.lock().unwrap();
                assert!(seen.refreshes.len() <= expected_refreshes);
                if seen.active && seen.hub.active && seen.refreshes.len() == expected_refreshes {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    let seen = mock.0.lock().unwrap();
    assert!(
        recovered.is_ok(),
        "hub={hub}, expected_refreshes={expected_refreshes}, refreshes={}, reports={:?}, relay={}, hub_active={}",
        seen.refreshes.len(),
        seen.reports,
        seen.active,
        seen.hub.active,
    );
}

#[tokio::test(start_paused = true)]
async fn persistent_hub_ticket_refusal_is_not_hidden_by_successful_renewals() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.error = Some(Error::AccessRejected);
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("persistent hub ticket rejection must terminate")
        .unwrap();
    assert_eq!(result, Err(Error::Unauthorized));
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
}
