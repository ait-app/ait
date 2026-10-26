use std::fmt::Write as _;

use super::*;

#[derive(Clone, Default)]
struct Logs(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for Logs {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }

    fn enter(&self, _span: &tracing::span::Id) {}

    fn exit(&self, _span: &tracing::span::Id) {}
}

struct Fields(String);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        write!(&mut self.0, " {}={value:?}", field.name()).unwrap();
    }
}

impl Logs {
    fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(self.clone())
    }

    fn text(&self) -> String {
        self.0.lock().unwrap().join("\n")
    }

    fn assert_closed(&self, reason: &str, generation: &str) {
        let text = self.text();
        let lines: Vec<_> = text
            .lines()
            .filter(|line| line.contains("Voice resources released"))
            .collect();
        assert_eq!(lines.len(), 1, "{text}");
        assert!(lines[0].contains(&format!("reason=\"{reason}\"")), "{text}");
        assert!(
            lines[0].contains(&format!("voice_id={generation}")),
            "{text}"
        );
        assert!(lines[0].contains("agent_id=\"agent-1\""), "{text}");
    }
}

#[tokio::test]
async fn logs_distinguish_client_stop_abort_and_connection_drop_without_transcripts() {
    for reason in ["client_stop", "client_abort", "connection_dropped"] {
        let logs = Logs::default();
        let dispatch = logs.dispatch();
        let _guard = tracing::dispatcher::set_default(&dispatch);
        let mut fixture = Fixture::new();
        let endpoint = native(&mut fixture, false);
        fixture.mode(true).await;
        let generation = fixture.connection.voice.generation.clone();
        endpoint
            .output
            .try_send(Event::Transcript(
                "private transcript must stay private".into(),
            ))
            .unwrap();
        fixture.poll();
        match reason {
            "client_stop" => {
                fixture.mode(false).await;
                fixture.mode(false).await;
                drop(fixture.connection);
            }
            "client_abort" => {
                fixture
                    .connection
                    .request("voice.abort.request", json!({}), &fixture.service)
                    .await
                    .unwrap();
                drop(fixture.connection);
            }
            "connection_dropped" => drop(fixture.connection),
            _ => unreachable!(),
        }
        assert!(endpoint.cancel.is_cancelled());
        logs.assert_closed(reason, &generation);
        assert!(!logs.text().contains("private transcript"));
    }
}

#[tokio::test(start_paused = true)]
async fn logs_playback_timeout_before_clearing_queue_state() {
    let logs = Logs::default();
    let dispatch = logs.dispatch();
    let _guard = tracing::dispatcher::set_default(&dispatch);
    let mut fixture = Fixture::new();
    let endpoint = native(&mut fixture, false);
    fixture.mode(true).await;
    let generation = fixture.connection.voice.generation.clone();
    endpoint.output.try_send(queued_audio(1)).unwrap();
    fixture.poll();
    tokio::time::advance(Duration::from_secs(31)).await;
    fixture.poll();
    drop(fixture.connection);

    logs.assert_closed("playback_ack_timeout", &generation);
    let text = logs.text();
    assert!(text.contains("pending_acknowledgments=1"), "{text}");
    assert!(text.contains("oldest_ack_ms=31000"), "{text}");
    assert!(text.contains("reason=\"speech_timeout\""), "{text}");
}

#[tokio::test]
async fn logs_distinguish_native_output_failure_close_and_disconnect() {
    for reason in ["native_closed", "native_failed", "native_output_closed"] {
        let logs = Logs::default();
        let dispatch = logs.dispatch();
        let _guard = tracing::dispatcher::set_default(&dispatch);
        let mut fixture = Fixture::new();
        let endpoint = native(&mut fixture, false);
        fixture.mode(true).await;
        let generation = fixture.connection.voice.generation.clone();
        match reason {
            "native_closed" => endpoint.output.try_send(Event::Closed).unwrap(),
            "native_failed" => endpoint.output.try_send(Event::Failed).unwrap(),
            "native_output_closed" => drop(endpoint.output),
            _ => unreachable!(),
        }
        fixture.poll();
        drop(fixture.connection);
        logs.assert_closed(reason, &generation);
    }
}

#[tokio::test]
async fn logs_playback_overflow_before_clearing_saturated_queues() {
    let logs = Logs::default();
    let dispatch = logs.dispatch();
    let _guard = tracing::dispatcher::set_default(&dispatch);
    let (mut fixture, endpoint) = saturated_playback().await;
    let generation = fixture.connection.voice.generation.clone();
    endpoint.output.try_send(queued_audio(1)).unwrap();
    fixture.poll();
    drop(fixture.connection);

    logs.assert_closed("playback_queue_full", &generation);
    assert!(logs.text().contains("playback_queue=32"));
}

#[tokio::test]
async fn logs_distinguish_saturated_and_closed_native_input() {
    for reason in ["native_input_queue_full", "native_input_closed"] {
        let logs = Logs::default();
        let dispatch = logs.dispatch();
        let _guard = tracing::dispatcher::set_default(&dispatch);
        let mut fixture = Fixture::new();
        let endpoint = native(&mut fixture, false);
        fixture.mode(true).await;
        let generation = fixture.connection.voice.generation.clone();
        if reason == "native_input_closed" {
            drop(endpoint.input);
        }
        let chunks = if reason == "native_input_queue_full" {
            17
        } else {
            1
        };
        for _ in 0..chunks {
            fixture.event(
                "voice.audio.chunk",
                json!({"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}),
            );
        }
        drop(fixture.connection);
        logs.assert_closed(reason, &generation);
    }
}
