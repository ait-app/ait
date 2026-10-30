use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use super::*;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

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
    fn subscriber(&self) -> impl tracing::Subscriber + Send + Sync + 'static {
        let capture = self.clone();
        tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || capture.clone())
            .finish()
    }

    fn text(&self) -> String {
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
