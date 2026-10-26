use std::{io::Write, sync::Mutex};

use tracing::Instrument;

use super::*;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<u8>>>);

impl Write for Logs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn media_cancellation_preserves_the_agent_context_and_records_its_source() {
    let logs = Logs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::INFO)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let span = tracing::info_span!("native_voice", agent_id = "fixture-agent");
    let (session, endpoint) = model::realtime::channel(&CancellationToken::new());
    let media = open(endpoint).instrument(span).await.unwrap();
    drop(session);
    let text = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
            if text.contains("Codex realtime media stopped") {
                break text;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("media cancellation should produce a lifecycle diagnostic");
    drop(media);

    let lines: Vec<_> = text
        .lines()
        .filter(|line| line.contains("Codex realtime media stopped"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(lines[0].contains("reason=\"caller_cancelled\""), "{text}");
    assert!(lines[0].contains("agent_id=\"fixture-agent\""), "{text}");
}
