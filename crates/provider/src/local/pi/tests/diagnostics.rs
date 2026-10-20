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
