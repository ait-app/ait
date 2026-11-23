use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use model::outbound::{Frame, Outbound};
use model::{Context, Lifecycle, Limits, Request, Runtime, ServerInfo, VERSION};
use provider_codex::testing::Fixture;
use serde_json::{Value, json};

use super::{request, wait};
use crate::dispatch::State;
use crate::test_support::worker_with_client;

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

#[tokio::test]
async fn spawned_agent_responses_keep_request_span_without_logging_payload() {
    let fixture = Fixture::new();
    let (execution, _) = worker_with_client(
        &fixture,
        model::creation::Creations::default(),
        Box::new(fixture.client()),
    );
    let runtime = Arc::new(Runtime::new(ServerInfo {
        server_id: "server".into(),
        instance_id: "instance".into(),
        version: None,
        listen: "127.0.0.1:0".into(),
        lifecycle: Lifecycle::Ready,
        protocol: VERSION,
        capabilities: Vec::new(),
        implemented_capabilities: Vec::new(),
        features: Vec::new(),
        limits: Limits::default(),
    }));
    let state = State {
        runtime: runtime.clone(),
        agents: None,
        skills: None,
        agent_runtime: None,
        agent_execution: Some(execution.clone()),
        summary_source: None,
        directory_changes: None,
        has_terminals: false,
    };
    let capture = Capture::default();
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);

    for method in ["agent.message.send.request", "agent.finish.wait.request"] {
        let (outbound, mut receiver) = Outbound::new();
        let params = json!({"agentId":"missing-agent","text":"private-payload"});
        let span = tracing::info_span!(
            "rpc",
            method,
            request_id = "request-42",
            agent_id = "missing-agent"
        );
        span.in_scope(|| {
            if method == "agent.finish.wait.request" {
                wait("request-42".into(), params, &state, &outbound).unwrap();
            } else {
                request(
                    Context {
                        request: Request {
                            id: "request-42".into(),
                            method: method.into(),
                            params,
                        },
                        runtime: &runtime,
                        outbound: &outbound,
                        available_subscriptions: 16,
                    },
                    &state,
                )
                .unwrap();
            }
        });
        let Frame::Text(message) = receiver.recv().await.unwrap().message else {
            panic!("expected JSON response");
        };
        let response: Value = serde_json::from_str(&message).unwrap();
        assert_eq!(response["type"], "error");
        assert_eq!(response["request_id"], "request-42");
    }

    let bytes = capture.0.lock().unwrap().clone();
    let log = String::from_utf8(bytes).unwrap();
    for method in ["agent.message.send.request", "agent.finish.wait.request"] {
        let line = log
            .lines()
            .find(|line| line.contains("RPC failed") && line.contains(method))
            .unwrap_or_else(|| panic!("missing error for {method}: {log}"));
        for field in ["request-42", "missing-agent"] {
            assert!(line.contains(field), "missing {field}: {line}");
        }
    }
    assert!(!log.contains("private-payload"));
    execution.shutdown().await.unwrap();
}
