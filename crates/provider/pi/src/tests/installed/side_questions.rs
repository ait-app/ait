//! Opt-in real Pi BTW test: owned official binary, isolated loopback model,
//! never user credentials or a paid model service.

use std::collections::BTreeMap;
use std::time::Duration;

use axum::{Json, Router, routing::post};
use serde_json::json;

use super::*;
use crate::side;
use provider_core::ports::agent_session::AgentSessionSpec;
use provider_core::ports::side_questions::{SideAsk, SideQuestions, SideRunnerSpec};
use provider_core::protocol::side_questions::{
    SideContextEntry, SideContextSnapshot, SideStrategy, SideTopic, SideTurnStatus,
};
use std::sync::{Arc, Mutex};
use tokio_util::task::AbortOnDropHandle;

/// Capture request bodies and answer every side question deterministically.
async fn loopback_model() -> (
    std::net::SocketAddr,
    Arc<Mutex<Vec<Value>>>,
    AbortOnDropHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            let answer = {
                let mut requests = observed.lock().unwrap();
                requests.push(request);
                format!("SIDE-ANSWER-{}", requests.len())
            };
            async move {
                let chunks = [
                    json!({"id":"test","object":"chat.completion.chunk","created":1,
                        "model":"fixture-model","choices":[{"index":0,
                        "delta":{"role":"assistant","content":answer},"finish_reason":null}]}),
                    json!({"id":"test","object":"chat.completion.chunk","created":1,
                        "model":"fixture-model","choices":[{"index":0,"delta":{},
                        "finish_reason":"stop"}],
                        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
                ];
                let body = format!(
                    "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    chunks[0], chunks[1]
                );
                ([("content-type", "text/event-stream")], body)
            }
        }),
    );
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    (address, requests, server)
}

fn snapshot() -> SideContextSnapshot {
    SideContextSnapshot {
        v: 1,
        agent_id: "agent".into(),
        provider: "pi".into(),
        model: Some("ait-fixture/fixture-model".into()),
        native_session_id: Some("main-native".into()),
        native_leaf_id: None,
        captured_at: 1,
        entries: vec![
            SideContextEntry {
                role: "user".into(),
                text: "FROZEN-SNAPSHOT user question".into(),
            },
            SideContextEntry {
                role: "assistant".into(),
                text: "FROZEN-SNAPSHOT assistant answer".into(),
            },
        ],
        truncated: false,
    }
}

fn ask(question: &str) -> SideAsk {
    SideAsk {
        request_id: format!("request-{question}"),
        question: question.into(),
        record_id: None,
        snapshot: snapshot(),
        previous: None,
    }
}

async fn settled(runner: &mut side::Runner) -> SideTopic {
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let topics = runner.side_history().await.unwrap();
            if let Some(topic) = topics.first()
                && topic.latest_turn().status != SideTurnStatus::Running
            {
                return topic.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("installed Pi side turn did not settle")
}

#[tokio::test]
#[ignore = "requires AIT_TEST_PI_BIN; isolated loopback model and owned native home"]
async fn installed_pi_side_questions_isolate_context_and_leave_no_native_state() {
    let binary = std::env::var_os("AIT_TEST_PI_BIN").expect("set AIT_TEST_PI_BIN");
    let (address, requests, _server) = loopback_model().await;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let native = root.join("native");
    let project = root.join("project");
    std::fs::create_dir_all(&native).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        native.join("models.json"),
        json!({"providers":{"ait-fixture":{
            "baseUrl":format!("http://{address}/v1"),"api":"openai-completions",
            "apiKey":"fixture-only",
            "models":[{"id":"fixture-model","name":"Fixture model","reasoning":false,
                "input":["text"],
                "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},
                "contextWindow":8192,"maxTokens":128}]
        }}})
        .to_string(),
    )
    .unwrap();
    // A project extension must never load in the auxiliary process.
    let extensions = project.join(".pi/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(
        extensions.join("probe.js"),
        "import {writeFileSync} from 'node:fs';
         writeFileSync('extension-executed', 'yes');
         export default function (pi) { pi.registerTool?.({name: 'probe_tool'}); }",
    )
    .unwrap();
    let mut client = PiClient::new(binary.into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "PI_CODING_AGENT_DIR".into(),
            native.to_str().unwrap().into(),
        ),
        ("HOME".into(), root.to_str().unwrap().into()),
        ("PI_TELEMETRY".into(), "0".into()),
    ]))
    .unwrap();
    client.session_environment = history::SessionEnvironment::isolated(&root, &native);
    let spec = SideRunnerSpec {
        session: AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: project.to_str().unwrap().into(),
            config: StoredAgentConfig {
                model: Some("ait-fixture/fixture-model".into()),
                ..StoredAgentConfig::default()
            },
            confinement: None,
        },
        directory: root.join("btw"),
        environment: AgentEnvironment::default(),
    };
    let mut runner = side::Runner::new(client.clone(), &spec).unwrap();
    runner.probe().await.unwrap();
    assert!(runner.side_capability().available);
    assert_eq!(runner.side_strategy(), SideStrategy::Isolated);

    runner.side_ask(ask("first side question")).await.unwrap();
    let first = settled(&mut runner).await;
    assert_eq!(first.latest_turn().status, SideTurnStatus::Complete);
    assert_eq!(first.latest_turn().answer, "SIDE-ANSWER-1");

    let follow = SideAsk {
        request_id: "request-follow".into(),
        question: "second side question".into(),
        record_id: Some(first.record.id.clone()),
        snapshot: snapshot(),
        previous: Some(first.clone()),
    };
    runner.side_ask(follow).await.unwrap();
    let second = settled(&mut runner).await;
    assert_eq!(second.record.turn_count(), 2);
    assert_eq!(second.latest_turn().answer, "SIDE-ANSWER-2");
    runner.close_side().await.unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    for request in requests.iter() {
        // Public denial flags remove tool exposure instead of emptying it.
        assert!(request.get("tools").is_none(), "{}", request);
        let body = request.to_string();
        assert!(body.contains("FROZEN-SNAPSHOT user question"));
        assert!(body.contains("FROZEN-SNAPSHOT assistant answer"));
    }
    let replay = requests[1].to_string();
    assert!(replay.contains("first side question"));
    assert!(replay.contains("SIDE-ANSWER-1"));
    assert!(replay.contains("second side question"));
    // No native session, no project extension, no copied configuration.
    assert!(!native.join("sessions").exists());
    assert!(!project.join("extension-executed").exists());
    assert!(!root.join("btw").exists());
}
