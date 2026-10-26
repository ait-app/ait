//! Installed Codex, isolated native state, and a loopback Responses fixture; no account or API key.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Router, extract::State, routing::post};
use tokio_util::task::AbortOnDropHandle;

use super::*;

async fn response(State(requests): State<Arc<AtomicUsize>>) -> impl axum::response::IntoResponse {
    let index = requests.fetch_add(1, Ordering::SeqCst) + 1;
    let text = format!("Offline response {index}");
    let message = json!({"id":format!("msg_{index}"),"type":"message","role":"assistant",
        "status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
    let response = json!({"id":format!("resp_{index}"),"object":"response","status":"completed",
        "output":[message],"usage":{"input_tokens":10,"output_tokens":3,"total_tokens":13,
        "input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}});
    let events = [
        (
            "response.created",
            json!({"response":{"id":format!("resp_{index}"),"status":"in_progress","output":[]}}),
        ),
        (
            "response.output_item.added",
            json!({"output_index":0,"item":{"id":format!("msg_{index}"),"type":"message","role":"assistant","content":[],"status":"in_progress"}}),
        ),
        (
            "response.output_text.delta",
            json!({"item_id":format!("msg_{index}"),"output_index":0,"content_index":0,"delta":text}),
        ),
        (
            "response.output_item.done",
            json!({"output_index":0,"item":message}),
        ),
        ("response.completed", json!({"response":response})),
    ];
    let body = events
        .into_iter()
        .fold(String::new(), |mut body, (kind, mut event)| {
            event["type"] = json!(kind);
            write!(&mut body, "event: {kind}\ndata: {event}\n\n").unwrap();
            body
        });
    ([("content-type", "text/event-stream")], body)
}

#[tokio::test]
#[ignore = "requires installed Codex and loopback binding; uses no external model or credentials"]
async fn installed_codex_offline_turns_rewind_and_unarchive_use_the_current_protocol() {
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new()
        .route("/v1/responses", post(response))
        .with_state(requests.clone());
    let _server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("codex-home");
    let cwd = root.path().join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        home.join("config.toml"),
        format!(
            "model = \"offline-model\"\nmodel_provider = \"offline\"\n\
        [model_providers.offline]\nname = \"Offline fixture\"\n\
        base_url = \"http://{address}/v1\"\nwire_api = \"responses\"\n\
        requires_openai_auth = false\nsupports_websockets = false\n"
        ),
    )
    .unwrap();
    let mut client = CodexClient::new(
        std::env::var_os("AIT_SERVER_CODEX_BIN").map_or_else(|| "codex".into(), Into::into),
    );
    client.environment = serde_json::from_value(json!({"CODEX_HOME":home})).unwrap();
    let spec = AgentSessionSpec {
        provider: "codex".into(),
        cwd: cwd.to_str().unwrap().into(),
        config: StoredAgentConfig {
            model: Some("offline-model".into()),
            ..StoredAgentConfig::default()
        },
    };
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    for (index, prompt) in ["first", "second"].into_iter().enumerate() {
        session.start_turn(prompt, &spec.config).await.unwrap();
        let outcome = tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                match session.poll_turn().unwrap() {
                    Some(AgentTurnEvent::Completed(text)) => break text,
                    Some(AgentTurnEvent::Failed) => {
                        panic!("offline Codex failed: {:?}", session.failure_message())
                    }
                    _ => tokio::time::sleep(Duration::from_millis(5)).await,
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(outcome, Some(format!("Offline response {}", index + 1)));
    }
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    let target = history
        .iter()
        .find(|entry| entry.item["type"] == "user_message" && entry.item["text"] == "second")
        .unwrap()
        .item["messageId"]
        .as_str()
        .unwrap();
    let fork = client.rewind(&handle, &spec, target).await.unwrap();
    assert_ne!(fork.descriptor.provider_handle_id, handle.session_id);
    assert!(
        fork.entries
            .iter()
            .any(|entry| entry.item["text"] == "first")
    );
    assert!(
        !fork
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "second")
    );
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap(), history);
    let mut transport = client.launch_transport(&spec.cwd, false).unwrap();
    transport.initialize().await.unwrap();
    transport
        .request("thread/archive", json!({"threadId":handle.session_id}))
        .await
        .unwrap();
    transport.close().await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed.close().await.unwrap();
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}
