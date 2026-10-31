use std::io::{self, Write};
use std::sync::{Arc, Mutex, OnceLock};

use super::*;

/// Captured tracing output shared by Pi's adapter regression tests.
#[derive(Clone, Default)]
pub(super) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    /// Create a scoped tracing subscriber writing to this capture.
    pub(super) fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        // Tracing's single-dispatcher fast path can cache callsites as disabled when
        // parallel tests without a subscriber register them. Keep a silent dispatcher
        // alive so callsite interest accounts for every registered capture subscriber.
        static SILENT_DISPATCH: OnceLock<tracing::Dispatch> = OnceLock::new();
        SILENT_DISPATCH
            .get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default()));
        let capture = self.clone();
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || capture.clone())
            .finish()
    }

    /// Read the captured logs as UTF-8.
    pub(super) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn missing_executable_diagnostics_are_quiet_and_recover_after_installation() {
    let directory = tempfile::tempdir().unwrap();
    let client = PiClient::new(directory.path().join("pi"));
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());

    for _ in 0..3 {
        assert!(!client.is_available().await.unwrap());
        assert_eq!(
            client.diagnostic().await.unwrap(),
            "Pi executable is unavailable; check PATH or AIT_SERVER_PI_BIN."
        );
    }
    assert!(capture.text().is_empty(), "{}", capture.text());

    std::fs::write(&client.program, include_str!("fixtures/pi.py")).unwrap();
    std::fs::set_permissions(&client.program, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!client.is_available().await.unwrap());
    assert_eq!(
        client.diagnostic().await.unwrap(),
        "Pi executable is unavailable; check PATH or AIT_SERVER_PI_BIN."
    );
    assert!(capture.text().is_empty(), "{}", capture.text());

    std::fs::set_permissions(&client.program, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(client.is_available().await.unwrap());
    assert!(client.diagnostic().await.unwrap().contains("Pi RPC ready"));
    std::fs::set_permissions(&client.program, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!client.is_available().await.unwrap());
    assert_eq!(
        client.diagnostic().await.unwrap(),
        "Pi executable is unavailable; check PATH or AIT_SERVER_PI_BIN."
    );
    assert!(capture.text().is_empty(), "{}", capture.text());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failed_version_probes_are_shared_throttled_and_recover_after_repair() {
    let (directory, client, _spec) = fixture("version-malformed");
    let clone = client.clone();
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());

    let (available, cloned_available, diagnostic) = tokio::join!(
        client.is_available(),
        clone.is_available(),
        client.diagnostic()
    );
    assert!(!available.unwrap());
    assert!(!cloned_available.unwrap());
    assert!(diagnostic.unwrap().contains("version probe failed"));
    assert_eq!(
        capture.text().matches("Native provider diagnostic").count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\n"
    );

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(4)).await;
    assert!(!clone.is_available().await.unwrap());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\n"
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    assert!(!clone.is_available().await.unwrap());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\nprobe\n"
    );
    assert_eq!(
        capture.text().matches("Native provider diagnostic").count(),
        1
    );

    std::fs::write(
        &client.program,
        include_str!("fixtures/pi.py").replace("'not-a-version'", "'1.2.0'"),
    )
    .unwrap();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    assert!(client.is_available().await.unwrap());
    assert!(clone.diagnostic().await.unwrap().contains("Pi RPC ready"));
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\nprobe\nprobe\n"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn version_spawn_failures_preserve_os_facts_and_respect_probe_cooldown() {
    use crate::service::provider_catalog::Catalog;
    use model::session::SessionEvents;

    let (_directory, client, _spec) = fixture("");
    std::fs::write(&client.program, "#!/missing/ait-pi-interpreter\n").unwrap();
    let clone = client.clone();
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let expected = "Pi executable could not be started (error kind: NotFound, OS error: 2); check its interpreter, executable format, and execution permissions.";

    for _ in 0..2 {
        assert!(!client.is_available().await.unwrap());
        assert_eq!(clone.diagnostic().await.unwrap(), expected);
        assert_eq!(
            transport::version(&client).await,
            Err(AgentSessionError::Unavailable)
        );
    }
    let clients: BTreeMap<String, Arc<dyn AgentClient>> =
        BTreeMap::from([(PROVIDER.into(), Arc::new(clone) as Arc<dyn AgentClient>)]);
    let available = Catalog::default()
        .execute(
            &clients,
            &SessionEvents::default(),
            "provider.available.list.request",
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(available["providers"][0]["available"], false);
    assert_eq!(available["providers"][0]["error"], expected);
    let log = capture.text();
    assert_eq!(log.matches("Native provider diagnostic").count(), 1);
    assert_eq!(log.matches("Native provider spawn failed").count(), 1);
    assert!(log.contains("operation=\"spawn\""));
    assert!(log.contains("error_kind="));
    assert!(log.contains("os_error="));
    assert!(!log.contains("/missing/ait-pi-interpreter"));

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    assert!(!client.is_available().await.unwrap());
    let log = capture.text();
    assert_eq!(log.matches("Native provider diagnostic").count(), 1);
    assert_eq!(log.matches("Native provider spawn failed").count(), 2);

    std::fs::write(&client.program, include_str!("fixtures/pi.py")).unwrap();
    assert_eq!(client.diagnostic().await.unwrap(), expected);
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::time::resume();
    assert!(client.is_available().await.unwrap());
    assert!(client.diagnostic().await.unwrap().contains("Pi RPC ready"));
}

#[tokio::test]
async fn invalid_executable_format_has_launch_guidance_without_native_text() {
    let (_directory, client, _spec) = fixture("");
    let native_text = "not an executable: private installation details\n";
    std::fs::write(&client.program, native_text).unwrap();

    assert!(!client.is_available().await.unwrap());
    let message = client.diagnostic().await.unwrap();
    let error = io::Error::from_raw_os_error(8);
    assert!(message.starts_with("Pi executable could not be started"));
    assert!(message.contains(&format!("error kind: {:?}, OS error: 8", error.kind())));
    assert!(message.contains("interpreter, executable format, and execution permissions"));
    assert!(!message.contains("PATH"));
    assert!(!message.contains(client.program.to_str().unwrap()));
    assert!(!message.contains(native_text.trim()));
}

#[tokio::test(flavor = "current_thread")]
async fn hanging_version_probe_is_bounded_and_reused_by_catalog_refreshes() {
    use crate::service::provider_catalog::Catalog;
    use model::session::SessionEvents;

    let (directory, client, spec) = fixture("version-timeout");
    let clients: BTreeMap<String, Arc<dyn AgentClient>> =
        BTreeMap::from([(PROVIDER.into(), Arc::new(client) as Arc<dyn AgentClient>)]);
    let catalog = Catalog::default();
    let events = SessionEvents::default();
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());

    tokio::time::timeout(Duration::from_secs(8), async {
        for _ in 0..2 {
            let refreshed = catalog
                .execute(
                    &clients,
                    &events,
                    "provider.snapshot.refresh.request",
                    json!({"cwd": spec.cwd}),
                )
                .await
                .unwrap();
            assert_eq!(refreshed["acknowledged"], true);
            let snapshot = catalog
                .execute(
                    &clients,
                    &events,
                    "provider.snapshot.get.request",
                    json!({"cwd": spec.cwd}),
                )
                .await
                .unwrap();
            assert_eq!(snapshot["entries"][0]["status"], "unavailable");
            assert!(
                snapshot["entries"][0]["error"]
                    .as_str()
                    .unwrap()
                    .contains("timed out")
            );
        }
    })
    .await
    .expect("catalog refreshes must share the five-second probe without repeating it");
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\n"
    );
    assert_eq!(capture.text().lines().count(), 1);
    assert!(capture.text().contains("category=\"timeout\""));
}

#[tokio::test]
async fn environment_override_has_an_independent_version_probe_cache() {
    let (directory, client, spec) = fixture("version-malformed");
    assert!(!client.is_available().await.unwrap());
    let environment = AgentEnvironment::try_from(BTreeMap::from([(
        "PI_FIXTURE_SCENARIO".into(),
        String::new(),
    )]))
    .unwrap();

    let mut session = client
        .create_session_with_environment(&spec, &environment)
        .await
        .unwrap();
    session.close().await.unwrap();
    assert!(!client.is_available().await.unwrap());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("versions.log")).unwrap(),
        "probe\nprobe\n"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn history_task_panics_are_logged_before_classification() {
    let capture = Capture::default();
    let _subscriber = tracing::subscriber::set_default(capture.subscriber());
    let error = tokio::task::spawn_blocking(|| panic!("history panic diagnostic"))
        .await
        .unwrap_err();

    assert_eq!(
        history_task_failed("list_sessions", &error),
        AgentSessionError::Failed
    );
    let log = capture.text();
    assert!(log.contains("operation=\"list_sessions\""));
    assert!(log.contains("history panic diagnostic"));
}
