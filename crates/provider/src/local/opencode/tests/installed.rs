//! Real native CLI acceptance with isolated XDG directories and a loopback-only model.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use tokio::sync::mpsc;
use tokio_util::task::AbortOnDropHandle;

use super::*;
use crate::service::agent_manager::ownership::Owners;

mod robustness;
mod workflows;

#[derive(Debug)]
struct Installed {
    root: TempDir,
    client: OpenCodeClient,
    spec: AgentSessionSpec,
    _server: AbortOnDropHandle<()>,
    requests: mpsc::UnboundedReceiver<Value>,
}

#[derive(Debug)]
struct Model {
    calls: AtomicUsize,
    requests: mpsc::UnboundedSender<Value>,
}

async fn installed(question: bool) -> Installed {
    let binary = std::env::var("AIT_TEST_OPENCODE_BIN").expect("set AIT_TEST_OPENCODE_BIN");
    let root = tempfile::tempdir().unwrap();
    let cwd = root
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut client = OpenCodeClient::new(binary.into());
    client.environment = BTreeMap::from([
        ("XDG_DATA_HOME".into(), format!("{cwd}/data")),
        ("XDG_CONFIG_HOME".into(), format!("{cwd}/config")),
        ("XDG_CACHE_HOME".into(), format!("{cwd}/cache")),
        ("XDG_STATE_HOME".into(), format!("{cwd}/state")),
        ("OPENCODE_TEST_HOME".into(), cwd.clone()),
        ("OPENCODE_DB".into(), format!("{cwd}/opencode.db")),
        ("OPENCODE_CLI_CONFIG_CONTENT".into(), "{}".into()),
        ("OPENCODE_CONFIG_DIR".into(), format!("{cwd}/config")),
        ("OPENCODE_CONFIG_CONTENT".into(), "{}".into()),
        ("OPENCODE_DISABLE_AUTOUPDATE".into(), "true".into()),
        ("OPENCODE_DISABLE_MODELS_FETCH".into(), "true".into()),
    ]);
    let version = launcher::version(&client, &cwd).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (sender, requests) = mpsc::unbounded_channel();
    let state = Arc::new(Model {
        calls: AtomicUsize::new(usize::from(!question)),
        requests: sender,
    });
    let router = Router::new()
        .route("/v1/chat/completions", post(model))
        .with_state(state);
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let configuration = if version.starts_with("1.") {
        json!({"model":"local/test-model","provider":{"local":{"name":"Local test","npm":"@ai-sdk/openai-compatible",
            "options":{"baseURL":format!("http://{address}/v1"),"apiKey":"local-only"},
            "models":{"test-model":{"name":"Test","limit":{"context":32000,"output":2000}}}}}})
    } else {
        json!({"model":"local/test-model","providers":{"local":{"name":"Local test","package":"@opencode/ai/providers/openai-compatible",
            "settings":{"baseURL":format!("http://{address}/v1"),"apiKey":"local-only"},
            "models":{"test-model":{"name":"Test","limit":{"context":32000,"output":2000}}}}}})
    };
    std::fs::write(root.path().join("opencode.json"), configuration.to_string()).unwrap();
    let spec = AgentSessionSpec {
        provider: "opencode".into(),
        cwd,
        config: StoredAgentConfig {
            model: Some("local/test-model".into()),
            ..Default::default()
        },
    };
    Installed {
        root,
        client,
        spec,
        _server: server,
        requests,
    }
}

async fn model(State(state): State<Arc<Model>>, Json(request): Json<Value>) -> Response {
    let _ = state.requests.send(request.clone());
    if let Some(response) = controlled_response(&request).await {
        return response;
    }
    let calls = &state.calls;
    if request["messages"]
        .to_string()
        .contains("Legacy ACP question")
    {
        assert!(!request["tools"].as_array().is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool["function"]["name"] == "question")
        }));
    }
    if request["messages"]
        .to_string()
        .contains("Summarize metadata")
    {
        assert!(request["tools"].as_array().is_none_or(Vec::is_empty));
    }
    let is_question_prompt = request["messages"]
        .to_string()
        .contains("Ask which language")
        || request["messages"].to_string().contains("Request shell")
        || request["messages"].to_string().contains("Run shell");
    if request["messages"].to_string().contains("Request shell")
        && request["tools"]
            .as_array()
            .is_some_and(|tools| !tools.is_empty())
    {
        assert!(
            request["messages"]
                .to_string()
                .contains("Native override sentinel")
        );
    }
    let tool_name = if request["messages"].to_string().contains("Request shell")
        || request["messages"].to_string().contains("Run shell")
    {
        if request["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|tool| tool["function"]["name"] == "bash"))
        {
            "bash"
        } else {
            "shell"
        }
    } else {
        "question"
    };
    let has_tool_result = request["messages"]
        .as_array()
        .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"));
    let has_question_tool = request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == tool_name)
    });
    let question = has_question_tool
        && is_question_prompt
        && !has_tool_result
        && calls.load(Ordering::SeqCst) == 0;
    if is_question_prompt && has_question_tool {
        calls.fetch_add(1, Ordering::SeqCst);
    }
    if tool_name == "question"
        && !question
        && request["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|message| message["role"] == "tool"))
    {
        assert!(
            request["messages"].to_string().contains("Rust"),
            "native form answer must reach the model"
        );
    }
    let delta = if question {
        let arguments = if matches!(tool_name, "shell" | "bash") {
            let command = if is_user_message(&request, "Run shell multiline approval") {
                format!("# {}\npwd", "approval comment ".repeat(80))
            } else {
                "pwd".into()
            };
            json!({"command":command,"description":"Show working directory"})
        } else {
            json!({"questions":[{"header":"Language","question":"Which language?","options":[{"label":"Rust","description":"Use Rust"},{"label":"Go","description":"Use Go"}]}]})
        }.to_string();
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"question_local","type":"function","function":{"name":tool_name,"arguments":arguments}}]})
    } else {
        json!({"role":"assistant","content":"authoritative answer"})
    };
    streamed_response(&delta, question)
}

fn streamed_response(delta: &Value, tool_call: bool) -> Response {
    let chunks = [
        json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
        json!({"choices":[{"index":0,"delta":{},"finish_reason":if tool_call {"tool_calls"} else {"stop"}}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
    ];
    (
        [("content-type", "text/event-stream")],
        format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            chunks[0], chunks[1]
        ),
    )
        .into_response()
}

fn is_user_message(request: &Value, text: &str) -> bool {
    // Native title generation repeats the user text, but omits attachments and tools.
    // Observe only the primary agent request when checking a user workflow.
    if request["messages"].as_array().is_some_and(|messages| {
        messages.iter().any(|message| {
            message["role"] == "system"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("You are a title generator."))
        })
    }) {
        return false;
    }
    let matches_text = |content: &Value| {
        content
            .as_str()
            .and_then(|content| content.strip_prefix(text))
            .is_some_and(|remaining| remaining.is_empty() || remaining.starts_with('\n'))
    };
    request["messages"]
        .as_array()
        .and_then(|messages| {
            messages
                .iter()
                .rev()
                .find(|message| message["role"] == "user")
        })
        .is_some_and(|message| {
            matches_text(&message["content"])
                || message["content"].as_array().is_some_and(|blocks| {
                    blocks
                        .iter()
                        .any(|block| block["type"] == "text" && matches_text(&block["text"]))
                })
        })
}

async fn controlled_response(request: &Value) -> Option<Response> {
    if let Some(response) = robustness::model_response(request) {
        return Some(response);
    }
    if is_user_message(request, "Reject model request") {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({"error":{"type":"FreeTierError",
                "message":"OpenCode's free tier can only be used from within OpenCode"}})),
            )
                .into_response(),
        );
    }
    if is_user_message(request, "Hold native request") {
        return std::future::pending().await;
    }
    if is_user_message(request, "Reject native authentication") {
        return Some((StatusCode::UNAUTHORIZED, Json(json!({"error":{"message":"local authentication rejected","type":"authentication_error"}}))).into_response());
    }
    None
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_five_sessions_and_native_model_error_preserve_history_and_reason() {
    let Installed {
        root: _root,
        client,
        spec,
        _server,
        ..
    } = installed(false).await;
    let mut sessions = Vec::new();
    let owners = Owners::default();
    for index in 0..5 {
        let session = client.create_session(&spec).await.unwrap();
        let lane = format!("agent-{index}");
        let record = serde_json::from_value(json!({
            "id":lane,"provider":"opencode","cwd":spec.cwd,
            "createdAt":"2026-10-10T00:00:00Z","updatedAt":"2026-10-10T00:00:00Z",
            "persistence":session.persistence()
        }))
        .unwrap();
        owners.bind(&lane, &record).unwrap();
        sessions.push(session);
    }
    let session = sessions.last_mut().unwrap();
    session
        .start_input(
            &crate::protocol::prompt::AgentPrompt {
                text: "Reject model request".into(),
                client_message_id: Some("model-error-input".into()),
                ..Default::default()
            },
            &spec.config,
        )
        .await
        .unwrap();
    let mut history = None;
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::History(entries) => history = Some(entries),
            AgentTurnEvent::Failed => break,
            AgentTurnEvent::RuntimeInfo(_)
            | AgentTurnEvent::Timeline(_)
            | AgentTurnEvent::Usage(_) => {}
            unexpected => panic!("unexpected event: {unexpected:?}"),
        }
    }
    let history = history.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].item["clientMessageId"], "model-error-input");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let expected = if launcher::version(&client, &spec.cwd)
        .await
        .unwrap()
        .starts_with("1.")
    {
        "OpenCode's free tier can only be used from within OpenCode"
    } else {
        "Authentication required: provider authentication required"
    };
    assert!(
        session.failure_message().unwrap().contains(expected),
        "native failure: {:?}",
        session.failure_message()
    );
    assert_replay(&history, &client.history(&handle, &spec.cwd).await.unwrap());
    for session in &mut sessions {
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated XDG and deterministic loopback model"]
async fn installed_acp_multi_turn_native_history_resume_and_discovery() {
    let Installed {
        root: _root,
        client,
        spec,
        _server,
        ..
    } = installed(false).await;
    let mut session = client.create_session(&spec).await.unwrap();
    let mut items = Vec::new();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    for index in 0..2 {
        session
            .start_input(
                &crate::protocol::prompt::AgentPrompt {
                    text: format!("hello {index}"),
                    client_message_id: Some(format!("client-{index}")),
                    ..Default::default()
                },
                &spec.config,
            )
            .await
            .unwrap();
        items = projected_turn(session.as_mut(), &timeline).await;
        let rows = timeline.read("agent").unwrap().1;
        assert_eq!(rows.len(), (index + 1) * 2);
        for pair in rows.as_chunks::<2>().0 {
            assert_eq!(pair[0].entry.item["type"], "user_message");
            assert_eq!(pair[1].entry.item["text"], "authoritative answer");
            assert!(pair[0].entry.timestamp <= pair[1].entry.timestamp);
        }
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_replay(&items, &client.history(&handle, &spec.cwd).await.unwrap());
    let listed = client
        .list_sessions(&crate::ports::native_history::ListOptions {
            cwd: Some(spec.cwd.clone()),
            scan_limit: 10,
        })
        .await
        .unwrap();
    assert!(
        listed
            .iter()
            .any(|entry| entry.provider_handle_id == handle.session_id)
    );
    let models = client.discover(&spec.cwd).await.unwrap();
    assert!(
        models
            .models
            .iter()
            .any(|model| model["id"] == "local/test-model")
    );
    let mut resumed = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    let before = timeline.read("agent").unwrap().1;
    resumed
        .start_turn("after restart", &spec.config)
        .await
        .unwrap();
    projected_turn(resumed.as_mut(), &timeline).await;
    let after = timeline.read("agent").unwrap().1;
    assert_eq!(after.len(), 6);
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(before.entry.timestamp, after.entry.timestamp);
    }
    resumed.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+ in AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_native_question_form_reaches_user_and_model() {
    let Installed {
        root: _root,
        client,
        spec,
        _server,
        ..
    } = installed(true).await;
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Ask which language to use, then continue.", &spec.config)
        .await
        .unwrap();
    let request = loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) if request["kind"] == "question" => {
                break request;
            }
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("native question did not reach client")
            }
            _ => {}
        }
    };
    let question = request["input"]["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|question| {
            question["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["label"] == "Rust"))
        })
        .unwrap();
    let key = question["header"].as_str().unwrap();
    let answer = if question["answerFormat"] == "array" {
        json!(["Rust"])
    } else {
        json!("Rust")
    };
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{key:answer}}}),
        )
        .await
        .unwrap();
    completed(session.as_mut()).await;
    assert_eq!(session.pending_permissions(), [] as [Value; 0]);
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_shell_output_is_text_in_live_and_replayed_history() {
    let Installed {
        root: _root,
        client,
        mut spec,
        _server,
        ..
    } = installed(true).await;
    spec.config.feature_values = Some(BTreeMap::from([("permission".into(), json!("allow"))]));
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_input(
            &crate::protocol::prompt::AgentPrompt {
                text: "Run shell tool, then continue.".into(),
                client_message_id: Some("shell-input".into()),
                ..Default::default()
            },
            &spec.config,
        )
        .await
        .unwrap();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let items = recorded_turn(session.as_mut(), &timeline, &turn).await;
    let (_, rows) = timeline.read("agent").unwrap();
    assert_eq!(rows[0].entry.item["type"], "user_message");
    assert_eq!(rows[0].entry.item["clientMessageId"], "shell-input");
    assert_eq!(
        rows.iter()
            .filter(|row| row.entry.item["type"] == "tool_call")
            .count(),
        1
    );
    assert!(
        rows.iter()
            .all(|row| row.entry.turn_id.as_deref() == Some(turn.as_str()))
    );
    let tool = items
        .iter()
        .find(|entry| entry.item["detail"]["type"] == "shell")
        .unwrap();
    assert_eq!(
        tool.item["detail"]["output"].as_str().unwrap().trim(),
        spec.cwd
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    assert_replay(&items, &history);
    let replayed = history.iter().find(|entry| entry.key == tool.key).unwrap();
    assert_eq!(
        replayed.item["detail"]["output"],
        tool.item["detail"]["output"]
    );
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_native_permission_rejection_and_next_turn() {
    let Installed {
        root: _root,
        client,
        mut spec,
        _server,
        ..
    } = installed(true).await;
    spec.config.feature_values = Some(BTreeMap::from([("permission".into(), json!("ask"))]));
    spec.config.system_prompt = Some("Native override sentinel".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Request shell tool, then continue.", &spec.config)
        .await
        .unwrap();
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::PermissionRequested(request) => {
                assert_eq!(request["kind"], "tool");
                session
                    .respond_permission(
                        request["id"].as_str().unwrap(),
                        &json!({"behavior":"deny"}),
                    )
                    .await
                    .unwrap();
                break;
            }
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled | AgentTurnEvent::Failed => {
                panic!("native permission did not reach client")
            }
            _ => {}
        }
    }
    loop {
        match event(session.as_mut()).await {
            AgentTurnEvent::Completed(_) | AgentTurnEvent::Cancelled => break,
            AgentTurnEvent::Failed => panic!("native rejection failed to settle"),
            _ => {}
        }
    }
    let mut rejected = spec.config.clone();
    rejected.model = Some("local/missing-model".into());
    assert_eq!(
        session.start_turn("rejected", &rejected).await.unwrap_err(),
        AgentSessionError::Rejected
    );
    spec.config.feature_values = Some(BTreeMap::from([("permission".into(), json!("deny"))]));
    session.start_turn("follow up", &spec.config).await.unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires OpenCode 1.18.4+ in AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_without_native_forms_disables_unanswerable_questions() {
    let Installed {
        root: _root,
        mut client,
        spec,
        _server,
        ..
    } = installed(false).await;
    let version = launcher::version(&client, &spec.cwd).await.unwrap();
    assert!(version.starts_with("1."));
    client
        .environment
        .insert("OPENCODE_ENABLE_QUESTION_TOOL".into(), "true".into());
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("Legacy ACP question: choose a language.", &spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    session.close().await.unwrap();
    let options = crate::ports::native_history::ListOptions {
        cwd: Some(spec.cwd.clone()),
        scan_limit: 10,
    };
    let before = client.list_sessions(&options).await.unwrap();
    assert_eq!(
        client
            .generate_summary(&spec, "metadata", &json!({"type":"object"}))
            .await
            .unwrap_err(),
        AgentSessionError::Unavailable
    );
    assert_eq!(
        client.list_sessions(&options).await.unwrap().len(),
        before.len()
    );
}

#[tokio::test]
#[ignore = "requires OpenCode 2.0.25+ in AIT_TEST_OPENCODE_BIN; isolated XDG and loopback model"]
async fn installed_acp_auxiliary_model_has_no_tools_and_removes_native_history() {
    let Installed {
        root: _root,
        client,
        spec,
        _server,
        ..
    } = installed(false).await;
    let options = crate::ports::native_history::ListOptions {
        cwd: Some(spec.cwd.clone()),
        scan_limit: 10,
    };
    assert_eq!(
        client.list_sessions(&options).await.unwrap(),
        [] as [crate::ports::native_history::SessionDescriptor; 0]
    );
    assert_eq!(
        client
            .generate_summary(&spec, "Summarize metadata", &json!({"type":"object"}))
            .await
            .unwrap(),
        "authoritative answer"
    );
    assert_eq!(
        client.list_sessions(&options).await.unwrap(),
        [] as [crate::ports::native_history::SessionDescriptor; 0]
    );
}

#[tokio::test]
#[ignore = "requires a broken OpenCode 2.0.0-2.0.24 in AIT_TEST_OPENCODE_BIN"]
async fn installed_acp_broken_v2_catalog_reports_an_upgrade_without_creating_a_session() {
    let Installed {
        root: _root,
        client,
        spec,
        _server,
        ..
    } = installed(false).await;
    let version = launcher::version(&client, &spec.cwd).await.unwrap();
    assert!(version.starts_with("2."));
    assert!(!launcher::supported(&version));

    assert!(!client.is_available().await.unwrap());
    assert!(
        client
            .diagnostic()
            .await
            .unwrap()
            .contains("Upgrade OpenCode to 2.0.25 or newer")
    );
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Unavailable)
    ));
}
