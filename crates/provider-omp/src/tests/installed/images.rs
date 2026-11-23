//! Images for a text-only model, left to the official peer (ADR-248), driven by a loopback
//! mock model that records what it receives.
use std::sync::{Arc, Mutex};

use super::*;

/// A valid 1x1 PNG; OMP re-encodes images, so they must decode.
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

/// Chat completion requests the mock model received.
pub(super) type Requests = Arc<Mutex<Vec<Value>>>;

/// Launch the installed peer in a fresh agent directory with a text-only mock model that
/// records every request and answers with plain text.
pub(super) async fn installed_with_recording_model() -> (Installed, Requests) {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let requests = Requests::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(mock_recorded_completion),
        )
        .with_state(Arc::clone(&requests));
    let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from(
            [
                ("PI_CODING_AGENT_DIR", agent.to_str().unwrap()),
                ("OMP_PROFILE", ""),
                ("PI_PROFILE", ""),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
        ))
        .unwrap(),
        ..OmpClient::new(program.into()).with_image_directory(root.path().join("images"))
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
    let installed = Installed {
        root,
        client,
        spec,
        _server: server,
    };
    (installed, requests)
}

/// Record the request and answer with plain text.
async fn mock_recorded_completion(
    axum::extract::State(requests): axum::extract::State<Requests>,
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    requests.lock().unwrap().push(request);
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":{"role":"assistant","content":"Fixture complete"},
            "finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_describes_images_to_a_text_only_model_in_a_hidden_message() {
    let (fixture, requests) = installed_with_recording_model().await;
    let prompt = AgentPrompt {
        text: "describe".to_owned(),
        images: vec![PromptImage {
            data: PNG.to_owned(),
            mime_type: "image/png".to_owned(),
        }],
        ..AgentPrompt::default()
    };
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();

    session
        .start_input(&prompt, &fixture.spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let live: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(entry.clone()),
            _ => None,
        })
        .collect();
    let user = live
        .iter()
        .find(|entry| entry.item["type"] == "user_message")
        .unwrap_or_else(|| panic!("{live:?}"));
    assert!(
        user.item["text"].to_string().contains("![Image]"),
        "{user:?}"
    );
    assert!(!format!("{live:?}").contains("vision"), "{live:?}");
    // The model sees OMP's description and an omission notice, never the image or a path
    // Ait wrote.
    // OMP may also ask the model to summarize discovered skills; find the prompt.
    let sent = requests
        .lock()
        .unwrap()
        .iter()
        .map(|request| request["messages"].to_string())
        .find(|messages| messages.contains("describe"))
        .unwrap();
    assert!(
        sent.contains("No vision-capable model is configured"),
        "{sent}"
    );
    assert!(sent.contains("local://image-"), "{sent}");
    assert!(!sent.contains("image_url"), "{sent}");
    assert!(!sent.contains("Image available at"), "{sent}");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    assert!(!format!("{history:?}").contains("vision"), "{history:?}");
    let recorded: Vec<_> = history
        .iter()
        .filter(|entry| entry.item["type"] != "user_message")
        .collect();
    let shown: Vec<_> = live
        .iter()
        .filter(|entry| entry.item["type"] != "user_message")
        .collect();
    assert_eq!(recorded, shown);
}
