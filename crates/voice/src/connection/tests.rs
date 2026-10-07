mod dictation;
mod dictation_lifecycle;
mod voice;
mod voice_lifecycle;

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use base64::{Engine as Base64Engine, engine::general_purpose::STANDARD};
use model::{
    Lifecycle, Limits, ServerInfo, VERSION,
    outbound::{Frame, Queued},
};

use crate::{
    audio::Format,
    ports::{Agents, Operation, Synthesizer, Transcriber},
};

use super::*;

#[test]
fn unavailable_and_draining_servers_reject_speech_events_without_allocating_streams() {
    for (available, expected) in [(false, "unsupported_capability"), (true, "server_draining")] {
        let mut fixture = Fixture::new();
        let state = crate::dispatch::State {
            runtime: fixture.runtime.clone(),
            speech: available.then(|| fixture.service.clone()),
        };
        if available {
            state.runtime.cancellation.cancel();
        }
        fixture
            .connection
            .event(
                "dictation.stream.start",
                serde_json::json!({"id":"stream","format":"pcm;rate=16000"}),
                &state,
                &fixture.outbound,
            )
            .unwrap();
        let queued = fixture.receiver.try_recv().unwrap();
        let Frame::Text(text) = queued.message else {
            panic!("expected error response")
        };
        let error: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(error["type"], "error");
        assert_eq!(error["code"], expected);
        assert!(fixture.connection.is_empty());
        assert_eq!(fixture.engine.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn unavailable_speech_requests_keep_correlation_and_do_not_emit_success_events() {
    for (available, expected) in [(false, "unsupported_capability"), (true, "server_draining")] {
        let mut fixture = Fixture::new();
        let state = crate::dispatch::State {
            runtime: fixture.runtime.clone(),
            speech: available.then(|| fixture.service.clone()),
        };
        if available {
            state.runtime.cancellation.cancel();
        }
        let mut context = Some(model::Context {
            request: model::Request {
                id: "speech-request".to_owned(),
                method: "voice.abort.request".to_owned(),
                params: json!({}),
            },
            runtime: &fixture.runtime,
            outbound: &fixture.outbound,
            available_subscriptions: 0,
        });
        crate::dispatch::dispatch(&mut context, &state, &mut fixture.connection)
            .await
            .unwrap();
        assert!(context.is_none());
        let Frame::Text(text) = fixture.receiver.try_recv().unwrap().message else {
            panic!("expected correlated error");
        };
        let response: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(response["request_id"], "speech-request");
        assert_eq!(response["code"], expected);
        assert!(fixture.receiver.try_recv().is_err());
        assert!(fixture.connection.is_empty());
    }
}

#[derive(Debug, Default)]
struct Engine {
    calls: AtomicUsize,
    cancelled: AtomicUsize,
    blocking: AtomicBool,
    preparing: AtomicBool,
    failing: AtomicBool,
    agent_blocking: AtomicBool,
    samples: Mutex<Vec<Vec<u8>>>,
    transcript: Mutex<Option<String>>,
    agent_calls: AtomicUsize,
    synthesis_calls: AtomicUsize,
    synthesis_blocking: AtomicBool,
    synthesis_failing: AtomicBool,
    agent_failing: AtomicBool,
    empty_reply: AtomicBool,
    resolve_blocking: AtomicBool,
}

impl Transcriber for Engine {
    fn readiness(&self) -> Result<(), Error> {
        if self.preparing.load(Ordering::SeqCst) {
            Err(Error::Preparing)
        } else {
            Ok(())
        }
    }
    fn transcribe(&self, audio: Audio, cancel: CancellationToken) -> Operation<'_, Transcript> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.samples.lock().unwrap().push(audio.bytes);
            if self.failing.load(Ordering::SeqCst) {
                return Err(Error::Provider);
            }
            if self.blocking.load(Ordering::SeqCst) {
                cancel.cancelled().await;
                self.cancelled.fetch_add(1, Ordering::SeqCst);
                return Err(Error::Cancelled);
            }
            Ok(Transcript {
                text: self
                    .transcript
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "recognized speech".to_owned()),
                language: Some("en".to_owned()),
            })
        })
    }
}

impl Synthesizer for Engine {
    fn synthesize<'a>(&'a self, text: &'a str, cancel: CancellationToken) -> Operation<'a, Audio> {
        Box::pin(async move {
            self.synthesis_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(text, "Agent reply");
            if self.synthesis_failing.load(Ordering::SeqCst) {
                return Err(Error::Provider);
            }
            if self.synthesis_blocking.load(Ordering::SeqCst) {
                cancel.cancelled().await;
                self.cancelled.fetch_add(1, Ordering::SeqCst);
                return Err(Error::Cancelled);
            }
            Ok(Audio {
                bytes: vec![1; 150_000],
                format: Format::Pcm(24000),
            })
        })
    }
}

impl Agents for Engine {
    fn resolve<'a>(&'a self, identifier: &'a str) -> Operation<'a, String> {
        Box::pin(async move {
            if self.resolve_blocking.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if identifier == "bad" {
                Err(Error::Agent)
            } else {
                Ok("agent-1".to_owned())
            }
        })
    }

    fn turn<'a>(
        &'a self,
        agent: &'a str,
        text: &'a str,
        cancel: CancellationToken,
    ) -> Operation<'a, String> {
        Box::pin(async move {
            self.agent_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(agent, "agent-1");
            if self.agent_failing.load(Ordering::SeqCst) {
                return Err(Error::Agent);
            }
            assert_eq!(text, "recognized speech");
            if self.agent_blocking.load(Ordering::SeqCst) {
                cancel.cancelled().await;
                self.cancelled.fetch_add(1, Ordering::SeqCst);
                return Err(Error::Cancelled);
            }
            Ok(if self.empty_reply.load(Ordering::SeqCst) {
                " \n ".to_owned()
            } else {
                "Agent reply".to_owned()
            })
        })
    }
}

struct Fixture {
    connection: Connection,
    service: Speech,
    runtime: Arc<Runtime>,
    outbound: Outbound,
    receiver: mpsc::Receiver<Queued>,
    engine: Arc<Engine>,
    events: Vec<Value>,
}

impl Fixture {
    fn new() -> Self {
        let engine = Arc::new(Engine::default());
        let service = Speech::new(
            Some(engine.clone()),
            Some(engine.clone()),
            Some(engine.clone()),
        );
        let runtime = Arc::new(Runtime::new(ServerInfo {
            server_id: "test".to_owned(),
            instance_id: "instance".to_owned(),
            version: None,
            listen: "127.0.0.1:1".to_owned(),
            lifecycle: Lifecycle::Ready,
            protocol: VERSION,
            capabilities: Vec::new(),
            implemented_capabilities: Vec::new(),
            features: Vec::new(),
            limits: Limits::default(),
        }));
        let (outbound, receiver) = Outbound::new();
        Self {
            connection: Connection::default(),
            service,
            runtime,
            outbound,
            receiver,
            engine,
            events: Vec::new(),
        }
    }

    fn event(&mut self, method: &str, params: Value) {
        let state = crate::dispatch::State {
            runtime: self.runtime.clone(),
            speech: Some(self.service.clone()),
        };
        self.connection
            .event(method, params, &state, &self.outbound)
            .unwrap();
        self.drain();
    }

    fn drain(&mut self) {
        while let Ok(frame) = self.receiver.try_recv() {
            let Frame::Text(text) = frame.message else {
                panic!("unexpected binary");
            };
            self.events.push(serde_json::from_str(&text).unwrap());
        }
    }

    fn poll(&mut self) {
        self.connection
            .poll(&self.service, &self.runtime, &self.outbound)
            .unwrap();
        self.drain();
    }

    async fn until(&mut self, method: &str) -> Value {
        for _ in 0..200 {
            self.poll();
            if let Some(index) = self
                .events
                .iter()
                .position(|event| event["method"] == method)
            {
                return self.events.remove(index)["params"].clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("missing {method}: {:?}", self.events);
    }

    fn start(&mut self, id: &str) {
        self.event(
            "dictation.stream.start",
            json!({"dictationId":id,"format":"audio/pcm;rate=16000;bits=16"}),
        );
    }

    fn chunk(&mut self, id: &str, seq: u32, bytes: &[u8]) {
        self.event("dictation.stream.chunk", json!({"dictationId":id,"seq":seq,"audio":STANDARD.encode(bytes),"format":"audio/pcm;rate=16000;bits=16"}));
    }

    async fn mode(&mut self, enabled: bool) -> Value {
        self.connection
            .request(
                "voice.mode.set.request",
                json!({"enabled":enabled,"agentId":"agent"}),
                &self.service,
            )
            .await
            .unwrap()
    }
}
