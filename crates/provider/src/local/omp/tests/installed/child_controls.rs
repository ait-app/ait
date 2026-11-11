//! Parent-owned child guide/stop against the installed OMP over loopback-only inference.
use super::*;
use crate::ports::controls::{ChildControlOutcome, ChildGuideOutcome, ChildStopOutcome};
use axum::body::Body;
use axum::response::Response;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; spawns a real child using loopback-only inference"]
async fn installed_omp_guides_and_cancels_a_real_native_child() {
    let fixture = installed_child_fixture().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("AIT_CHILD_CONTROL_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();
    let child = tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            if let Some(child) = session
                .subagents()
                .into_iter()
                .find(|child| child.descriptor["status"] == "running")
            {
                return child;
            }
            // The delegated child outlives the parent turn, so settlement keeps the wait alive.
            let _ = session.poll_turn().unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("installed OMP never published the delegated child");

    let guidance = session
        .guide_child(&child.id, "Ait guidance")
        .await
        .unwrap();
    assert_eq!(
        settled(session.as_mut(), &guidance.token).await,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::Accepted))
    );
    let stopped = session.stop_child(&child.id).await.unwrap();
    assert_eq!(
        settled(session.as_mut(), &stopped.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::Cancelled))
    );
    // Acceptance is not termination: termination arrives as an observed child status.
    tokio::time::timeout(Duration::from_secs(60), async {
        while session
            .subagents()
            .iter()
            .all(|entry| entry.id != child.id || entry.descriptor["status"] != "canceled")
        {
            session.poll_turn().unwrap();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the stopped child never reported a terminal status");
    // A terminal child is a not-running fact for both controls, without another native write.
    let stopped = session.stop_child(&child.id).await.unwrap();
    assert_eq!(
        settled(session.as_mut(), &stopped.token).await,
        ChildControlOutcome::Stop(Ok(ChildStopOutcome::NotRunning))
    );
    let guidance = session.guide_child(&child.id, "late").await.unwrap();
    assert_eq!(
        settled(session.as_mut(), &guidance.token).await,
        ChildControlOutcome::Guide(Ok(ChildGuideOutcome::NotRunning))
    );
    session.close().await.unwrap();
}

/// Service the control hook until the deferred typed answer for `token` arrives.
async fn settled(session: &mut dyn AgentSession, token: &str) -> ChildControlOutcome {
    loop {
        session.service_controls().await.unwrap();
        if let Some(event) = session.poll_turn().unwrap()
            && let AgentTurnEvent::ChildControl {
                token: event_token,
                outcome,
            } = event
            && event_token == token
        {
            return outcome;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    client: OmpClient,
    spec: AgentSessionSpec,
    _server: tokio_util::task::AbortOnDropHandle<()>,
}

async fn installed_child_fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(mock_child_completion),
    );
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    std::fs::write(agent.join("models.yml"),json!({"providers":{"ait-local-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}}).to_string()).unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                agent.to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into())
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            model: Some(json!(["ait-local-fixture", "fixture"]).to_string()),
            ..StoredAgentConfig::default()
        },
        confinement: None,
    };
    Fixture {
        _root: root,
        client,
        spec,
        _server: server,
    }
}

/// Answer the parent delegation prompt, then keep the child's own turn open until it is stopped.
async fn mock_child_completion(axum::Json(request): axum::Json<Value>) -> Response {
    let messages = &request["messages"];
    let finished = messages
        .as_array()
        .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"));
    let text = messages.to_string();
    let chunk = |delta: Value, finish: Value| {
        json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}],
            "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}})
    };
    let mut frames = Vec::new();
    // The parent's answer after the delegation tool result wins over every other marker.
    let hangs = !finished && text.contains("AIT_LOOPBACK_CHILD");
    if finished {
        frames.push(chunk(
            json!({"role":"assistant","content":"Parent done"}),
            json!("stop"),
        ));
    } else if hangs {
        frames.push(chunk(
            json!({"role":"assistant","content":"Child working"}),
            json!(null),
        ));
    } else {
        frames.push(chunk(
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture-child",
                "type":"function","function":{"name":"task","arguments":json!({
                    "context":"Shared loopback context.",
                    "tasks":[{"task":"AIT_LOOPBACK_CHILD keep working until stopped",
                        "description":"worker"}]})
                    .to_string()}}]}),
            json!("tool_calls"),
        ));
    }
    let total = frames.len();
    let stream = futures_util::stream::unfold(0_usize, move |index| {
        let frames = frames.clone();
        async move {
            if let Some(frame) = frames.get(index) {
                return Some((
                    Ok::<_, std::io::Error>(format!("data: {frame}\n\n")),
                    index + 1,
                ));
            }
            // A hanging child keeps its native turn open, so only the control can end it.
            if hangs && index == total {
                tokio::time::sleep(Duration::from_secs(120)).await;
            }
            None
        }
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("valid loopback response")
}
