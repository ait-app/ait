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

#[async_trait::async_trait(?Send)]
impl EvidenceSource for FixtureSources {
    async fn collect(&self, _start: DateTime<Utc>, _end: DateTime<Utc>) -> String {
        "Harness: opencode\nVersion: fixture\n".into()
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

#[test]
fn tracing_omits_oversized_fields_and_unapproved_payloads() {
    let (sender, _receiver) = mpsc::sync_channel(4);
    let diagnostics = Diagnostics {
        sender,
        recent: Arc::default(),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    let subscriber = tracing_subscriber::registry().with(diagnostics.clone());
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(error = %"x".repeat(2049), payload = "private body", "failure");
    });
    let recent = diagnostics.recent.lock().unwrap();
    assert_eq!(recent.events.len(), 1);
    let event = &recent.events[0].text;
    assert!(event.contains("failure"));
    assert!(event.contains("[oversized field omitted]"));
    assert!(!event.contains("private body"));
    assert!(!event.contains(&"x".repeat(2049)));
}

#[tokio::test]
async fn tracing_preserves_exhausted_budget_and_safe_event_context() {
    use tracing::Instrument;

    let (sender, _receiver) = mpsc::sync_channel(4);
    let diagnostics = Diagnostics {
        sender,
        recent: Arc::default(),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    let runtime = model::Runtime::new(model::ServerInfo {
        server_id: "server".into(),
        instance_id: "instance".into(),
        version: None,
        listen: "127.0.0.1:7316".into(),
        lifecycle: model::Lifecycle::Ready,
        protocol: model::VERSION,
        capabilities: Vec::new(),
        implemented_capabilities: Vec::new(),
        features: Vec::new(),
        limits: model::Limits::default(),
    });
    let _held = runtime.jobs.clone().acquire_many_owned(3).await.unwrap();
    let subscriber = tracing_subscriber::registry().with(diagnostics.clone());
    let _guard = tracing::subscriber::set_default(subscriber);
    let span = tracing::info_span!(
        "event",
        method = tracing::field::Empty,
        subscription_slots = 16,
        params = "private-event-body",
        token = "private-credential",
    );
    span.record("method", "session.heartbeat");

    assert_eq!(
        runtime
            .run_shared(
                Some(Arc::new(())),
                model::ErrorCode::RegistryIo,
                |()| Ok(())
            )
            .instrument(span)
            .await,
        Err(model::ErrorCode::ResourceExhausted)
    );
    let recent = diagnostics.recent.lock().unwrap();
    assert_eq!(recent.events.len(), 1);
    let event = &recent.events[0].text;
    for field in [
        "Resource budget exhausted",
        "resource=\"short_jobs\"",
        "limit=3",
        "available=0",
        "method=\"session.heartbeat\"",
        "subscription_slots=16",
    ] {
        assert!(event.contains(field), "{event}");
    }
    assert!(!event.contains("private-event-body"), "{event}");
    assert!(!event.contains("private-credential"), "{event}");
}

#[test]
fn tracing_preserves_provider_failure_facts_without_native_payloads() {
    let (sender, _receiver) = mpsc::sync_channel(4);
    let diagnostics = Diagnostics {
        sender,
        recent: Arc::default(),
        dropped: Arc::default(),
        storage_failures: Arc::default(),
    };
    let subscriber = tracing_subscriber::registry().with(diagnostics.clone());
    tracing::subscriber::with_default(subscriber, || {
        tracing::warn!(
            provider = "deepseek-harness",
            operation = "permissionPresets/catalog",
            source = "http_connect",
            category = "network",
            error_kind = "ConnectionRefused",
            os_error = 111,
            native_code = -32000,
            http_status = 503,
            will_retry = false,
            suppressed = 2,
            payload = "private body",
            cookie = "private cookie",
            url = "http://127.0.0.1/?token=private",
            "Native provider diagnostic"
        );
    });
    let recent = diagnostics.recent.lock().unwrap();
    let event = &recent.events[0].text;
    for fact in [
        "deepseek-harness",
        "permissionPresets/catalog",
        "source=\"http_connect\"",
        "category=\"network\"",
        "error_kind=\"ConnectionRefused\"",
        "os_error=111",
        "native_code=-32000",
        "http_status=503",
        "will_retry=false",
        "suppressed=2",
    ] {
        assert!(event.contains(fact), "missing {fact}: {event}");
    }
    assert!(!event.contains("private"), "{event}");
    assert!(!event.contains("token"), "{event}");
}

struct OversizedSources;

#[async_trait::async_trait(?Send)]
impl EvidenceSource for OversizedSources {
    async fn collect(&self, _start: DateTime<Utc>, _end: DateTime<Utc>) -> String {
        "fixture evidence\n".repeat(REPORT_LIMIT)
    }
}

#[test]
fn manual_report_bounds_large_provider_evidence_at_a_complete_line() {
    let directory = tempfile::tempdir().unwrap();
    let diagnostics =
        Diagnostics::with_sources(directory.path().to_owned(), OversizedSources).unwrap();

    let report = diagnostics.report();

    let evidence = report.split("\nSaved incidents:").next().unwrap();
    assert!(evidence.len() <= REPORT_LIMIT);
    assert!(report.contains("fixture evidence"));
    assert!(report.contains("[report truncated at 256 KiB]"));
    assert!(!report.contains("timed out"));
}

#[test]
fn manual_report_exposes_unavailable_incident_storage_and_reuses_cached_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let invalid = directory.path().join("file");
    std::fs::write(&invalid, "fixture").unwrap();
    let diagnostics = Diagnostics::with_sources(invalid, FixtureSources).unwrap();

    let first = diagnostics.report();
    let second = diagnostics.report();

    assert!(first.contains("Incident storage unavailable"));
    assert!(first.contains("Harness: opencode"));
    assert_eq!(first, second);
}
