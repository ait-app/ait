//! Timeline markers, full journals and quarantine recovery across delivery attempts.

use super::*;

fn ingress(app: &Application, center: &FakeCenter) -> (Ingress, mpsc::Receiver<Incoming>) {
    let (sender, receiver) = mpsc::channel(INGRESS_EVENTS);
    let ingress = Ingress {
        state: center.state(),
        queue: sender,
        bytes: Arc::new(Semaphore::new(INGRESS_BYTES)),
        changes: app.sources.changes.clone(),
        native: true,
        overflowing: AtomicBool::new(false),
    };
    (ingress, receiver)
}

fn timeline(seq: u64) -> Value {
    json!({"agentId":"agent","epoch":"epoch","seq":seq})
}

fn status_event() -> Value {
    json!({"eventId":Uuid::new_v4(),"method":"status.server_info","params":{}})
}

/// Whether the journal holds no deliverable event; directory snapshots are ignored.
fn journal_is_empty(app: &Application, scope: &Scope) -> bool {
    app.outbox
        .lock()
        .unwrap()
        .next(scope, &[EVENTS.to_owned()], Utc::now().timestamp_millis())
        .is_none()
}

#[tokio::test]
async fn timeline_markers_skip_the_journal_coalesce_and_follow_journal_events() {
    let (app, center, store, _directory) = application();
    let scope = center.snapshot().scope.unwrap();
    app.outbox.lock().unwrap().activate(&scope, 0).unwrap();
    let (ingress, mut receiver) = ingress(&app, &center);
    for seq in 1..=3 {
        assert!(!ingress.publish("agent_stream", &timeline(seq)));
        app.store_event(&receiver.recv().await.unwrap())
            .await
            .unwrap();
    }
    assert!(journal_is_empty(&app, &scope));
    assert_eq!(store.document.lock().unwrap()["events"], json!([]));
    let journaled = status_event();
    app.outbox
        .lock()
        .unwrap()
        .enqueue(&scope, journaled.clone(), Utc::now().timestamp_millis())
        .unwrap();
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(!app.attempt(center.snapshot()).await.unwrap());
    let calls = center.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].2, journaled);
    assert_eq!(calls[1].1, EVENTS);
    assert_eq!(calls[1].2["method"], "agent.timeline.update");
    assert_eq!(calls[1].2["params"]["seq"], 3);
}

#[tokio::test]
async fn failed_marker_delivery_is_retried_and_permanent_rejection_drops_it() {
    let (app, center, _store, _directory) = application();
    let (ingress, mut receiver) = ingress(&app, &center);
    ingress.publish("agent_stream", &timeline(1));
    app.store_event(&receiver.recv().await.unwrap())
        .await
        .unwrap();
    center.results.lock().unwrap().extend([
        Err(CallError::Unavailable),
        Err(CallError::Rejected("invalid_params".to_owned())),
    ]);
    assert!(app.attempt(center.snapshot()).await.is_err());
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(!app.attempt(center.snapshot()).await.unwrap());
    let calls = center.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].2["eventId"], calls[1].2["eventId"]);
}

#[tokio::test]
async fn full_journal_drops_the_event_and_resynchronizes_instead_of_retrying() {
    let (app, center, _store, _directory) = application();
    let scope = center.snapshot().scope.unwrap();
    let now = Utc::now().timestamp_millis();
    {
        let mut outbox = app.outbox.lock().unwrap();
        outbox.activate(&scope, now).unwrap();
        while outbox.enqueue(&scope, status_event(), now).is_ok() {}
    }
    let (ingress, mut receiver) = ingress(&app, &center);
    let mut changes = app.sources.changes.subscribe();
    changes.mark_unchanged();
    ingress.publish("status.server_info", &json!({"version":"test"}));
    let incoming = receiver.recv().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), app.admit(&incoming))
        .await
        .expect("a full journal must not be retried");
    assert!(changes.has_changed().unwrap());
}

#[tokio::test]
async fn a_fresh_link_retries_events_quarantined_by_the_previous_link() {
    let (app, center, _store, _directory) = application();
    let scope = center.snapshot().scope.unwrap();
    app.outbox.lock().unwrap().activate(&scope, 0).unwrap();
    app.outbox
        .lock()
        .unwrap()
        .enqueue(&scope, status_event(), Utc::now().timestamp_millis())
        .unwrap();
    center
        .results
        .lock()
        .unwrap()
        .push_back(Err(CallError::Rejected("unauthorized".to_owned())));
    assert!(app.attempt(center.snapshot()).await.unwrap());
    assert!(journal_is_empty(&app, &scope));
    app.reconcile(center.snapshot(), false).await.unwrap();
    assert!(journal_is_empty(&app, &scope));
    app.reconcile(center.snapshot(), true).await.unwrap();
    assert!(!journal_is_empty(&app, &scope));
}
