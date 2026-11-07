use super::*;

#[tokio::test(start_paused = true)]
async fn late_ticket_error_cannot_revoke_a_committed_successor_jwt() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().access_seconds = Some(10);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..4 {
        tick().await;
    }
    {
        let mut seen = mock.0.lock().unwrap();
        seen.hub.active = false;
        seen.hub.ticket_delay = Duration::from_secs(2);
        seen.hub.error = Some(Error::Unauthorized);
    }
    tick().await;
    mock.0.lock().unwrap().renew_delay = Duration::from_secs(120);
    for _ in 0..3 {
        tick().await;
    }
    assert_eq!(mock.0.lock().unwrap().refreshes.len(), 2);
    assert!(!task.is_finished());
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn disconnected_hub_reopens_without_stopping_transport() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    tick().await; // Observe the established handshake before simulating a later disconnect.
    mock.0.lock().unwrap().hub.active = false;
    tick().await;
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.hub.starts, 2);
        assert_eq!(seen.stops, 0);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn timed_out_tickets_retry_without_interrupting_the_lease() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.ticket_delay = Duration::from_secs(120);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..12 {
        tick().await;
    }
    {
        let mut seen = mock.0.lock().unwrap();
        assert!(seen.renewals >= 11);
        assert!(!seen.hub.active);
        assert_eq!(seen.stops, 0);
        seen.hub.ticket_delay = Duration::ZERO;
    }
    for _ in 0..61 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert!(seen.hub.active);
        assert!(seen.hub.tickets >= 2);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn lease_expiry_cancels_a_ticket_ready_at_the_deadline() {
    let (controller, mock) = fixture();
    {
        let mut seen = mock.0.lock().unwrap();
        seen.lease_seconds = Some(5);
        seen.hub.ticket_delay = Duration::from_secs(5);
    }
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    mock.0.lock().unwrap().renew_delay = Duration::from_secs(120);
    for _ in 0..7 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert!(!seen.hub.active);
        assert_eq!(seen.hub.starts, 0);
        assert_eq!(seen.hub.tickets, 1);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn replacing_session_discards_old_tickets_and_starts_the_link() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.ticket_delay = Duration::from_secs(5);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    {
        let mut seen = mock.0.lock().unwrap();
        seen.renew_error = Some(Error::SessionExpired);
        seen.hub.ticket_delay = Duration::ZERO;
    }
    for _ in 0..7 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.registrations.len(), 2);
        assert_eq!(seen.hub.starts, 1);
        assert!(seen.hub.active);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn ticket_revocation_interrupts_inflight_renewal_and_closes_the_link() {
    let (controller, mock) = fixture();
    let task = tokio::spawn(controller.run(CancellationToken::new()));
    tick().await;
    tick().await;
    {
        let mut seen = mock.0.lock().unwrap();
        seen.renew_delay = Duration::from_secs(120);
        seen.hub.active = false;
        seen.hub.error = Some(Error::Unauthorized);
    }
    tick().await;
    tick().await;
    assert!(task.is_finished());
    assert_eq!(task.await.unwrap(), Err(Error::Unauthorized));
    let seen = mock.0.lock().unwrap();
    assert!(!seen.hub.active);
    assert_eq!(seen.reports.last(), Some(&"reauthorization_required"));
}

#[tokio::test(start_paused = true)]
async fn shutdown_cancels_outstanding_tickets_without_late_starts() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.ticket_delay = Duration::from_secs(5);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    cancel.cancel();
    task.await.unwrap().unwrap();
    for _ in 0..6 {
        tick().await;
    }
    let seen = mock.0.lock().unwrap();
    assert_eq!(seen.hub.starts, 0);
    assert_eq!(seen.closes, 1);
}

#[tokio::test(start_paused = true)]
async fn repeated_async_hub_handshake_failures_back_off_until_online_then_reset() {
    let (controller, mock) = fixture();
    mock.0.lock().unwrap().hub.connecting = true;
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    tick().await;
    // Starting an async connector is not a successful handshake. Each later failure must
    // retain the growing interval instead of getting a fresh ticket every second.
    for interval in [5, 10, 20, 40, 60, 60] {
        let attempts = mock.0.lock().unwrap().hub.tickets;
        mock.0.lock().unwrap().hub.active = false;
        for _ in 0..interval - 1 {
            tick().await;
        }
        assert_eq!(mock.0.lock().unwrap().hub.tickets, attempts);
        tick().await;
        assert_eq!(mock.0.lock().unwrap().hub.tickets, attempts + 1);
    }
    mock.0.lock().unwrap().hub.connecting = false;
    tick().await;
    mock.0.lock().unwrap().hub.active = false;
    tick().await;
    let attempts = mock.0.lock().unwrap().hub.tickets;
    mock.0.lock().unwrap().hub.connecting = true;
    mock.0.lock().unwrap().hub.active = false;
    for _ in 0..4 {
        tick().await;
    }
    assert_eq!(mock.0.lock().unwrap().hub.tickets, attempts);
    tick().await;
    assert_eq!(mock.0.lock().unwrap().hub.tickets, attempts + 1);
    assert!(mock.0.lock().unwrap().renewals > 100);
    cancel.cancel();
    task.await.unwrap().unwrap();
}
