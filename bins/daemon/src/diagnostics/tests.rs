use super::*;
use tracing_subscriber::prelude::*;

#[test]
fn sanitizer_omits_credentials_payloads_and_private_locations() {
    let safe = sanitize(
        "authorization=Bearer hidden\n{\"token\":\"hidden\"}\nprompt=private\nERROR failed user@example.test /home/private/log https://private/?key=hidden\nplain failure",
    );
    for secret in ["hidden", "user@example.test", "/home/private", "https://"] {
        assert!(!safe.contains(secret), "{safe}");
    }
    assert!(safe.contains("plain failure"));
}

#[test]
fn callback_bounds_events_and_coalesces_capture_without_io() {
    let (sender, receiver) = mpsc::sync_channel(4);
    let diagnostics = Diagnostics {
        sender,
        recent: Arc::new(Mutex::new(Recent::default())),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    let subscriber = tracing_subscriber::registry().with(diagnostics.clone());
    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("ignored");
        let (outbound, _receiver) = model::outbound::Outbound::new();
        outbound
            .respond("request-42".into(), Err(model::ErrorCode::AgentIo))
            .unwrap();
    });
    assert_eq!(diagnostics.recent.lock().unwrap().events.len(), 1);
    assert!(
        diagnostics.recent.lock().unwrap().events[0]
            .text
            .contains("request-42")
    );
    for _ in 0..200 {
        diagnostics.record("failure".into(), Utc::now());
    }
    assert_eq!(diagnostics.recent.lock().unwrap().events.len(), EVENT_LIMIT);
    assert_eq!(receiver.try_iter().count(), 1);
    assert!(diagnostics.recent.lock().unwrap().overwritten > 0);
}

#[test]
fn saturated_collector_reports_busy_and_counts_dropped_captures() {
    let (sender, _receiver) = mpsc::sync_channel(0);
    let diagnostics = Diagnostics {
        sender,
        recent: Arc::default(),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    diagnostics.record("failure".into(), Utc::now());
    assert_eq!(diagnostics.dropped.load(Ordering::Relaxed), 1);
    assert!(diagnostics.report().contains("busy"));
    let mut text = LimitedText(String::new());
    assert!(write!(text, "{}", "x".repeat(2049)).is_err());
    assert!(text.0.is_empty());
}

struct FixtureSources;

impl EvidenceSource for FixtureSources {
    fn collect(
        &self,
        _start: DateTime<Utc>,
        _end: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = String> + '_>> {
        Box::pin(async { "Harness: opencode\nVersion: fixture\n".into() })
    }
}

#[test]
fn worker_saves_sanitized_evidence_and_manual_report_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let diagnostics =
        Diagnostics::with_sources(directory.path().to_owned(), FixtureSources).unwrap();
    diagnostics.record("fixture failure".into(), Utc::now());
    let report = diagnostics.report();
    assert!(report.contains("fixture failure"));
    assert!(report.contains("Saved incidents: 1"));
    assert!(report.contains("Harness: opencode"));
    assert!(report.contains("Incident ID:"));
    assert!(report.contains("persistence failures: 0"));
    drop(diagnostics);
    let restarted = Diagnostics::with_sources(directory.path().to_owned(), FixtureSources).unwrap();
    assert!(restarted.report().contains("fixture failure"));
}

#[test]
fn deferred_capture_checks_new_events_and_exposes_storage_failures() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("not-a-directory");
    std::fs::write(&invalid, "fixture").unwrap();
    let now = Utc::now();
    let worker = Worker {
        directory: invalid,
        sources: Box::new(FixtureSources),
        recent: Arc::new(Mutex::new(Recent {
            events: VecDeque::from([Event {
                at: now,
                text: "failure".into(),
            }]),
            ..Recent::default()
        })),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    assert_eq!(
        worker.pending(now - chrono::Duration::seconds(1)),
        Some(now)
    );
    assert!(worker.pending(now).is_none());
    assert!(worker.pending(now - chrono::Duration::seconds(1)).is_none());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    worker.save(&runtime, now);
    assert_eq!(worker.storage_failures.load(Ordering::Relaxed), 1);
}
