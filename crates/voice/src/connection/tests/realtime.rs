use super::*;
use model::realtime::{Endpoint, Event, Session};

#[derive(Debug)]
struct Native {
    session: Mutex<Option<Session>>,
    rejection: Option<Error>,
}

impl Agents for Native {
    fn resolve<'a>(&'a self, _identifier: &'a str) -> Operation<'a, String> {
        Box::pin(async { Ok("agent-1".to_owned()) })
    }

    fn realtime<'a>(
        &'a self,
        _agent: &'a str,
        _cancel: CancellationToken,
    ) -> Operation<'a, Option<Session>> {
        Box::pin(async move {
            if let Some(error) = self.rejection {
                return Err(error);
            }
            Ok(self.session.lock().unwrap().take())
        })
    }

    fn turn<'a>(
        &'a self,
        _agent: &'a str,
        _text: &'a str,
        _cancel: CancellationToken,
    ) -> Operation<'a, String> {
        Box::pin(async { panic!("native audio must never become an Ait text turn") })
    }
}

fn native(fixture: &mut Fixture, reject: bool) -> Endpoint {
    let mut endpoint = pending_native(fixture, reject);
    let (ready, _) = tokio::sync::oneshot::channel();
    std::mem::replace(&mut endpoint.ready, ready)
        .send(())
        .unwrap();
    endpoint
}

fn pending_native(fixture: &mut Fixture, reject: bool) -> Endpoint {
    let (session, endpoint) = model::realtime::channel(&CancellationToken::new());
    fixture.service.agents = Some(Arc::new(Native {
        session: Mutex::new(Some(session)),
        rejection: reject.then_some(Error::Realtime),
    }));
    endpoint
}

#[tokio::test]
async fn native_busy_is_actionable_retryable_and_does_not_start_capture_or_fallback() {
    let mut fixture = Fixture::new();
    fixture.service.agents = Some(Arc::new(Native {
        session: Mutex::new(None),
        rejection: Some(Error::AgentBusy),
    }));
    let response = fixture.mode(true).await;
    assert_eq!(response["accepted"], false);
    assert_eq!(response["reasonCode"], "voice_agent_busy");
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("another client")
    );
    assert_eq!(response["retryable"], true);
    assert!(fixture.connection.is_empty());
    assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_audio_bypasses_speech_engines_and_keeps_ordered_playback() {
    let mut fixture = Fixture::new();
    let mut endpoint = native(&mut fixture, false);
    fixture.service.stt = None;
    fixture.service.tts = None;
    assert_eq!(fixture.mode(true).await["accepted"], true);
    fixture.event(
        "voice.audio.chunk",
        json!({"audio":"AAgACA==","format":"pcm;rate=16000","isLast":false}),
    );
    let audio = endpoint.input.try_recv().unwrap();
    assert_eq!(audio.bytes, [0, 8, 0, 8]);
    assert_eq!(audio.sample_rate, 16000);
    endpoint
        .output
        .try_send(Event::Transcript("native speech".to_owned()))
        .unwrap();
    for byte in [1, 2] {
        endpoint
            .output
            .try_send(Event::Audio(model::realtime::Audio {
                bytes: vec![byte; 4],
                sample_rate: 24000,
            }))
            .unwrap();
    }
    assert_eq!(
        fixture.until("voice.transcription.result").await["text"],
        "native speech"
    );
    for byte in [1, 2] {
        let output = fixture.until("voice.audio.output").await;
        assert_eq!(output["audio"], STANDARD.encode([byte; 4]));
        assert_eq!(output["format"], "audio/pcm;rate=24000;bits=16;channels=1");
    }
    assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.engine.synthesis_calls.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.engine.agent_calls.load(Ordering::SeqCst), 0);
    drop(fixture.connection);
    assert!(endpoint.cancel.is_cancelled());
}

#[tokio::test]
async fn native_start_failure_is_reported_even_when_generic_engines_are_ready() {
    let mut fixture = Fixture::new();
    let _endpoint = native(&mut fixture, true);
    let response = fixture.mode(true).await;
    assert_eq!(response["accepted"], false);
    assert_eq!(response["enabled"], false);
    assert_eq!(response["reasonCode"], "native_realtime_failed");
    assert!(fixture.connection.is_empty());
    assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_close_and_error_release_the_target_and_notify_the_client() {
    for failed in [false, true] {
        let mut fixture = Fixture::new();
        let endpoint = native(&mut fixture, false);
        fixture.mode(true).await;
        endpoint
            .output
            .try_send(if failed { Event::Failed } else { Event::Closed })
            .unwrap();
        fixture.until("voice.session.closed").await;
        if failed {
            assert_eq!(
                fixture.until("voice.error").await["reasonCode"],
                "native_realtime_failed"
            );
        }
        assert!(endpoint.cancel.is_cancelled());
        assert!(fixture.connection.is_empty());
        fixture.event(
            "voice.audio.chunk",
            json!({"audio":"AAgACA==","format":"pcm;rate=16000","isLast":true}),
        );
        assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn native_abort_cancels_audio_and_does_not_enable_generic_fallback() {
    let mut fixture = Fixture::new();
    let endpoint = native(&mut fixture, false);
    fixture.mode(true).await;
    fixture
        .connection
        .request("voice.abort.request", json!({}), &fixture.service)
        .await
        .unwrap();
    assert!(endpoint.cancel.is_cancelled());
    fixture.event(
        "voice.audio.chunk",
        json!({"audio":"AAgACA==","format":"pcm;rate=16000","isLast":true}),
    );
    assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn upstream_sdp_is_rejected_at_the_client_boundary() {
    let mut fixture = Fixture::new();
    let _endpoint = native(&mut fixture, false);
    assert!(
        fixture
            .connection
            .request(
                "voice.mode.set.request",
                json!({"enabled":true,"agentId":"agent","sdp":"client-offer"}),
                &fixture.service
            )
            .await
            .is_err()
    );
    assert!(fixture.connection.is_empty());
}

#[tokio::test]
async fn failed_or_expired_media_start_releases_the_native_session() {
    for expired in [false, true] {
        let mut fixture = Fixture::new();
        let endpoint = pending_native(&mut fixture, false);
        let Endpoint { ready, cancel, .. } = endpoint;
        let mut ready = Some(ready);
        if !expired {
            ready.take();
        }
        if expired {
            tokio::time::pause();
        }
        let response = fixture.mode(true).await;
        assert_eq!(response["accepted"], false);
        assert_eq!(response["reasonCode"], "native_realtime_failed");
        assert!(cancel.is_cancelled());
        assert!(fixture.connection.is_empty());
        drop(ready);
        if expired {
            tokio::time::resume();
        }
    }
}

#[tokio::test]
async fn native_pcm_window_tolerates_relay_latency_and_remains_acknowledgment_bounded() {
    let mut fixture = Fixture::new();
    let endpoint = native(&mut fixture, false);
    fixture.mode(true).await;
    for _ in 0..20 {
        endpoint
            .output
            .try_send(Event::Audio(model::realtime::Audio {
                bytes: vec![0; 3840],
                sample_rate: 24_000,
            }))
            .unwrap();
    }
    for _ in 0..10 {
        fixture.poll();
    }
    let audio: Vec<_> = fixture
        .events
        .iter()
        .filter(|event| event["method"] == "voice.audio.output")
        .collect();
    assert_eq!(audio.len(), 16);
    let first = audio[0]["params"]["id"].as_str().unwrap().to_owned();
    fixture.connection.voice.played(&first);
    fixture.events.clear();
    fixture.poll();
    assert_eq!(
        fixture
            .events
            .iter()
            .filter(|event| event["method"] == "voice.audio.output")
            .count(),
        1
    );
}
