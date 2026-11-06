use super::*;

fn declare(mock: &Mock, features: &[&str], lease: bool) {
    let mut seen = mock.0.lock().unwrap();
    seen.hub_features = Some(features.iter().map(|&name| name.to_owned()).collect());
    seen.register_hub_ticket = Some("registered");
    seen.hub.lease = lease;
}

#[tokio::test(start_paused = true)]
async fn old_centers_keep_relay_control_hub_tickets_and_http_renewal() {
    let (controller, mock) = fixture();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..4 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!((seen.starts, seen.ticket_requests), (1, 1));
        assert_eq!(seen.hub.started, ["hub-ticket-secret"]);
        assert!(seen.renewals >= 4, "{} renewals", seen.renewals);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn lease_and_download_replace_relay_control_and_use_the_registration_ticket() {
    let (controller, mock) = fixture();
    declare(
        &mock,
        &["lease", "hosts", "state", "credit", "download"],
        false,
    );
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..4 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!((seen.starts, seen.ticket_requests), (0, 0));
        assert_eq!(seen.hub.started, ["registered-1"]);
        assert_eq!(seen.hub.tickets, 0);
    }
    // A reconnect asks for a fresh ticket; the registration ticket is spent.
    mock.0.lock().unwrap().hub.active = false;
    for _ in 0..6 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.hub.started, ["registered-1", "hub-ticket-secret"]);
        assert_eq!(seen.starts, 0);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn relay_control_stays_when_the_center_lacks_download_or_lease() {
    for features in [&["lease", "credit"][..], &["download"][..]] {
        let (controller, mock) = fixture();
        declare(&mock, features, false);
        let cancel = CancellationToken::new();
        let task = tokio::spawn(controller.run(cancel.clone()));
        tick().await;
        assert_eq!(mock.0.lock().unwrap().starts, 1, "{features:?}");
        cancel.cancel();
        task.await.unwrap().unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn a_lease_holding_link_replaces_http_renewal_until_it_stops_holding() {
    let (controller, mock) = fixture();
    declare(&mock, &["lease", "credit", "download"], true);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..3 {
        tick().await;
    }
    let renewed = mock.0.lock().unwrap().renewals;
    assert_eq!(
        renewed, 1,
        "only the deadline after registration is renewed"
    );
    // Beyond the HTTP lease, the link keeps transport open without renewal.
    for _ in 0..90 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.renewals, 1);
        assert_eq!(seen.stops, 0);
        assert!(seen.hub.active);
    }

    // Without the link's lease, renewal resumes and an expired session registers again.
    {
        let mut seen = mock.0.lock().unwrap();
        seen.hub.lease = false;
        seen.renew_error = Some(Error::SessionExpired);
    }
    for _ in 0..3 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert!(seen.renewals >= 2);
        assert_eq!(seen.registrations.len(), 2);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_rotated_token_is_renewed_once_while_the_link_holds_the_lease() {
    let (controller, mock) = fixture();
    declare(&mock, &["lease", "credit", "download"], true);
    // Two-minute tokens rotate about every minute.
    mock.0.lock().unwrap().access_seconds = Some(120);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..200 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        // Each rotation reaches the center once so a device grant's deadline advances;
        // between rotations the link alone holds the lease.
        assert!(
            seen.refreshes.len() >= 3,
            "{} refreshes",
            seen.refreshes.len()
        );
        assert_eq!(seen.renewals, seen.refreshes.len());
        assert_eq!(seen.registrations.len(), 1);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn a_freeze_past_the_local_lease_keeps_the_lease_holding_link() {
    let (controller, mock) = fixture();
    declare(&mock, &["lease", "credit", "download"], true);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..3 {
        tick().await;
    }
    // A suspended laptop wakes 90 s later; the center still holds the link in grace.
    tokio::time::advance(Duration::from_secs(90)).await;
    for _ in 0..3 {
        tick().await;
    }
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.stops, 0, "transport stays up for the link to resume");
        assert!(seen.hub.active);
        assert_eq!(seen.registrations.len(), 1);
        assert_eq!(seen.renewals, 1);
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn hub_outages_without_relay_control_keep_the_manager_running() {
    let (controller, mock) = fixture();
    declare(&mock, &["lease", "download"], true);
    {
        let mut seen = mock.0.lock().unwrap();
        seen.register_hub_ticket = None;
        seen.hub.error = Some(Error::Unavailable);
    }
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..20 {
        tick().await;
    }
    assert!(!task.is_finished());
    {
        let seen = mock.0.lock().unwrap();
        assert!(seen.hub.tickets >= 2, "the Hub link keeps retrying");
        assert_eq!(seen.starts, 0);
        assert!(seen.renewals >= 10, "HTTP renewal continues without a link");
        assert_eq!(seen.reports.last(), Some(&"running"));
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn registration_tickets_expire_locally() {
    let ticket = crate::HubTicket {
        ticket: "registered".into(),
        features: None,
        expires_at: Some(tokio::time::Instant::now() + Duration::from_secs(30)),
    };
    assert!(ticket.usable());
    assert!(crate::HubTicket::new("immediate".into(), None).usable());
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(!ticket.usable());
}

#[tokio::test(start_paused = true)]
async fn bulk_tickets_present_the_latest_token_until_the_manager_stops() {
    let (controller, mock) = fixture();
    declare(&mock, &["lease", "credit", "download", "bulk"], true);
    // Two-minute tokens rotate about every minute while the link stays up.
    mock.0.lock().unwrap().access_seconds = Some(120);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(controller.run(cancel.clone()));
    for _ in 0..200 {
        tick().await;
    }
    let bulk = mock
        .0
        .lock()
        .unwrap()
        .hub
        .bulk
        .clone()
        .expect("hub started");
    let ticket = bulk.request().await.expect("bulk ticket");
    assert_eq!(ticket.ticket.expose_secret(), "bulk-ticket-secret");
    {
        let seen = mock.0.lock().unwrap();
        assert_eq!(seen.hub.starts, 1);
        assert!(seen.refreshes.len() >= 3);
        assert_eq!(
            seen.hub.bulk_tokens,
            [format!("device-access-secret-{}", seen.refreshes.len())]
        );
    }
    cancel.cancel();
    task.await.unwrap().unwrap();
    assert_eq!(bulk.request().await.err(), Some(Error::Unavailable));
}
