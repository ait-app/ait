//! Opt-in real Pi test against an isolated loopback model, never user credentials.

use super::*;
use crate::protocol::prompt::PromptImage;
use axum::{Json, Router, routing::post};
use std::sync::{Arc, Mutex};
use tokio_util::task::AbortOnDropHandle;

mod reasoning;

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; isolated native controls without model requests"]
async fn installed_pi_reasoning_controls_follow_settings_clamping_and_extensions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let native = root.join("native");
    let client = reasoning_client(&root, "http://127.0.0.1:1/v1");
    let mut spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    let model = details
        .models
        .iter()
        .find(|model| model["id"] == "ait-fixture/reasoner")
        .unwrap();
    assert_eq!(model["isDefault"], true);
    assert_eq!(model["defaultThinkingOptionId"], "low");
    assert_eq!(
        model["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["off", "minimal", "low", "high", "xhigh"]
    );
    assert!(!native.join("sessions").exists());

    // Mirror the composer: creation submits the catalog's advertised default.
    spec.config.thinking_option_id = model["defaultThinkingOptionId"].as_str().map(str::to_owned);
    let mut session = client.create_session(&spec).await.unwrap();
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("low")
    );
    // Pi clamps medium upward to high; readback must retain that effective value.
    spec.config.thinking_option_id = Some("medium".into());
    session
        .start_turn("/ait_think", &spec.config)
        .await
        .unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(events.iter().any(|event| matches!(event, AgentTurnEvent::RuntimeInfo(info) if native_thinking(info) == Some("minimal"))));
    assert_eq!(
        native_thinking(&session.runtime_info().await.unwrap()),
        Some("minimal")
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    spec.config.thinking_option_id = Some("medium".into());
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        native_thinking(&resumed.runtime_info().await.unwrap()),
        Some("high")
    );
    resumed.close().await.unwrap();

    // A stale profile's disabled max level remains admissible and resumable.
    spec.config.thinking_option_id = Some("max".into());
    client.validate_selection(&spec).await.unwrap();
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(
        native_thinking(&resumed.runtime_info().await.unwrap()),
        Some("xhigh")
    );
    resumed.close().await.unwrap();
    let settings: Value =
        serde_json::from_slice(&std::fs::read(native.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["defaultThinkingLevel"], "low");
}

fn reasoning_client(root: &std::path::Path, base_url: &str) -> PiClient {
    let native = root.join("native");
    let extensions = native.join("extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(
        native.join("models.json"),
        json!({"providers":{"ait-fixture":{
            "baseUrl":base_url,"api":"openai-completions","apiKey":"fixture-only",
            "models":[{"id":"reasoner","name":"Reasoner","reasoning":true,
                "thinkingLevelMap":{"medium":null,"xhigh":"high","max":null},
                "input":["text"],"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},
                "contextWindow":8192,"maxTokens":128}]
        }}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        native.join("settings.json"),
        json!({
            "defaultProvider":"ait-fixture","defaultModel":"reasoner","defaultThinkingLevel":"low"
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(extensions.join("thinking.js"),
        "export default function(pi) { pi.registerCommand('ait_think', { handler: async () => { pi.setThinkingLevel('minimal'); } }); }").unwrap();
    let mut client = PiClient::new("pi".into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "PI_CODING_AGENT_DIR".into(),
            native.to_str().unwrap().into(),
        ),
        ("HOME".into(), root.to_str().unwrap().into()),
    ]))
    .unwrap();
    client.session_environment = history::SessionEnvironment::isolated(root, &native);
    client
}

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; uses only an isolated loopback model"]
async fn installed_pi_streams_and_resumes_with_loopback_model() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let (address, requests, _server) = loopback_model().await;
    let native = root.join("native");
    std::fs::create_dir(&native).unwrap();
    std::fs::write(native.join("models.json"), json!({"providers":{"ait-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","apiKey":"fixture-only",
        "models":[{"id":"fixture-model","name":"Fixture model","reasoning":false,"input":["text","image"],
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"contextWindow":8192,"maxTokens":128}]
    }}}).to_string()).unwrap();
    // An undecided project must not execute local extensions during discovery/startup.
    let extensions = root.join(".pi/extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(extensions.join("trust.js"),
        "import {writeFileSync} from 'node:fs'; writeFileSync('project-executed', 'yes'); export default function() {};").unwrap();
    let mut client = PiClient::new("pi".into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "PI_CODING_AGENT_DIR".into(),
            native.to_str().unwrap().into(),
        ),
        ("HOME".into(), root.to_str().unwrap().into()),
    ]))
    .unwrap();
    client.session_environment = history::SessionEnvironment::isolated(&root, &native);
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.to_str().unwrap().into(),
        config: StoredAgentConfig {
            model: Some("ait-fixture/fixture-model".into()),
            ..StoredAgentConfig::default()
        },
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(
        details
            .models
            .iter()
            .any(|model| model["id"] == "ait-fixture/fixture-model")
    );
    assert!(!native.join("sessions").exists());
    let mut session = fresh_resumed_session(&client, &spec).await;
    session
        .start_turn("Exercise the tool cards", &spec.config)
        .await
        .unwrap();
    let events = terminal(session.as_mut()).await;
    assert!(
        matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello from real Pi"),
        "{events:?}"
    );
    assert_tool_cards(&events);
    assert_eq!(
        std::fs::read_to_string(root.join("card.txt")).unwrap(),
        "after\n"
    );
    let handle = session.persistence().unwrap();
    assert!(
        std::path::Path::new(handle.native_handle.as_ref().unwrap().as_str().unwrap()).is_file()
    );
    session.close().await.unwrap();
    let handle = import_handle(&client, &handle, &spec.cwd).await;
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    let image = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
    let prompt = AgentPrompt {
        images: vec![PromptImage {
            data: image.into(),
            mime_type: "image/png".into(),
        }],
        attachments: vec![
            json!({"type":"text","mimeType":"text/plain","text":"attachment evidence"}),
        ],
        ..AgentPrompt::text("Say hello again")
    };
    resumed.start_input(&prompt, &spec.config).await.unwrap();
    let events = terminal(resumed.as_mut()).await;
    assert!(
        matches!(events.last(),Some(AgentTurnEvent::Completed(Some(text))) if text == "Hello from real Pi"),
        "{events:?}"
    );
    resumed.close().await.unwrap();
    assert!(!root.join("project-executed").exists());
    let requests = requests.lock().unwrap();
    let rich = requests.last().unwrap().to_string();
    assert!(rich.contains("attachment evidence"));
    assert!(
        rich.contains("data:image/png;base64,"),
        "Model request omitted the valid PNG image"
    );
}

async fn import_handle(
    client: &PiClient,
    handle: &AgentPersistenceHandle,
    cwd: &str,
) -> AgentPersistenceHandle {
    let sessions = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(cwd.into()),
            scan_limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].provider_handle_id, handle.session_id);
    let mut imported = AgentPersistenceHandle {
        provider: PROVIDER.into(),
        session_id: handle.session_id.clone(),
        native_handle: None,
        metadata: None,
    };
    let history = client.inspect_session(&imported, cwd).await.unwrap();
    assert!(!history.active);
    assert_eq!(
        history.descriptor.first_prompt_preview.as_deref(),
        Some("Exercise the tool cards")
    );
    assert_eq!(
        history
            .entries
            .iter()
            .filter(|entry| entry.item["type"] == "tool_call")
            .count(),
        4
    );
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.item["text"] == "Hello from real Pi")
    );
    imported.metadata = Some(history.resume_metadata);
    imported
}

async fn loopback_model() -> (
    std::net::SocketAddr,
    Arc<Mutex<Vec<Value>>>,
    AbortOnDropHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(request): Json<Value>| {
        let step = {
            let mut requests = observed.lock().unwrap();
            let step = requests.len();
            requests.push(request);
            step
        };
        async move {
        let call = match step {
            0 => Some(("bash", json!({"command":"printf 'shell evidence\\n'"}))),
            1 => Some(("write", json!({"path":"card.txt","content":"before\n"}))),
            2 => Some(("edit", json!({"path":"card.txt","edits":[{"oldText":"before","newText":"after"}]}))),
            3 => Some(("bash", json!({"command":"printf 'failure evidence\\n'; exit 1"}))),
            _ => None,
        };
        let delta = if let Some((name, args)) = &call {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("card-{step}"),
                "type":"function","function":{"name":name,"arguments":args.to_string()}}]})
        } else {
            json!({"role":"assistant","content":"Hello from real Pi"})
        };
        let finish = if call.is_some() { "tool_calls" } else { "stop" };
        let chunks = [
            json!({"id":"test","object":"chat.completion.chunk","created":1,"model":"fixture-model",
                "choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
            json!({"id":"test","object":"chat.completion.chunk","created":1,"model":"fixture-model",
                "choices":[{"index":0,"delta":{},"finish_reason":finish}],
                "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
        ];
        let body = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", chunks[0], chunks[1]);
        ([("content-type","text/event-stream")],body)
    }}));
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    (address, requests, server)
}

async fn fresh_resumed_session(
    client: &PiClient,
    spec: &AgentSessionSpec,
) -> Box<dyn AgentSession> {
    let mut session = client.create_session(spec).await.unwrap();
    let handle = session.persistence().unwrap();
    let path = handle.native_handle.as_ref().unwrap().as_str().unwrap();
    assert!(std::path::Path::new(path).is_absolute());
    session.close().await.unwrap();
    let resumed = client
        .resume_session(&handle, spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    assert_eq!(resumed.persistence().unwrap().session_id, handle.session_id);
    resumed
}

fn assert_tool_cards(events: &[AgentTurnEvent]) {
    let cards: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                Some(&entry.item)
            }
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 4, "{events:?}");
    assert_eq!(cards[0]["detail"]["type"], "shell");
    assert_eq!(cards[0]["detail"]["output"], "shell evidence\n");
    assert_eq!(cards[0]["status"], "completed");
    assert_eq!(cards[1]["detail"]["type"], "write");
    assert_eq!(cards[1]["detail"]["content"], "before\n");
    assert_eq!(cards[1]["status"], "completed");
    assert_eq!(cards[2]["detail"]["type"], "edit");
    assert_eq!(cards[2]["detail"]["filePath"], "card.txt");
    let diff = cards[2]["detail"]["unifiedDiff"].as_str().unwrap();
    assert!(
        diff.contains("-before") && diff.contains("+after"),
        "{diff}"
    );
    assert!(diff.contains("@@ -"), "expected native patch, got {diff}");
    assert_eq!(cards[2]["status"], "completed");
    assert_eq!(cards[3]["status"], "failed");
    assert!(
        cards[3]["detail"]["output"]
            .as_str()
            .unwrap()
            .contains("failure evidence")
    );
}
