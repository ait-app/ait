use super::*;

mod interactions;
mod settlement;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; uses isolated configuration without model inference"]
async fn installed_omp_discovers_and_opens_native_rpc_without_inference() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":[{"id":"fixture","name":"Local fixture","reasoning":false,"input":["text"],
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let client = OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from([
            (
                "PI_CODING_AGENT_DIR".to_owned(),
                root.path().join("agent").to_str().unwrap().to_owned(),
            ),
            ("OMP_PROFILE".to_owned(), String::new()),
            ("PI_PROFILE".to_owned(), String::new()),
        ]))
        .unwrap(),
        ..OmpClient::new(program.into()).with_image_directory(root.path().join("images"))
    };
    let spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig {
            mode_id: Some("ask".to_owned()),
            ..StoredAgentConfig::default()
        },
    };
    assert!(client.is_available().await.unwrap());
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.modes.len(), 4);
    client.commands(&spec).await.unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let info = session.runtime_info().await.unwrap();
    assert_eq!(info.provider, "omp");
    assert_eq!(info.mode_id.as_deref(), Some("ask"));
    assert!(info.session_id.is_some());
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    assert_eq!(client.history(&handle, &spec.cwd).await.unwrap().len(), 0);
    let mut reopened = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    reopened.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; probes real native model capabilities without inference"]
async fn installed_omp_tiers_and_thinking_follow_model_without_changing_defaults() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let native_models = ["gpt-5.4", "deepseek-v4.1-flash"].map(|id| {
        json!({
            "id":id,"name":id,"reasoning":true,"input":["text"],
            "thinking":{"mode":"effort","efforts":["low","high","max"]},
            "contextWindow":32768,"maxTokens":1024,
            "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}})
    });
    let models = json!({"providers":{"ait-local-fixture":{
        "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
        "models":native_models}}});
    std::fs::write(agent.join("models.yml"), models.to_string()).unwrap();
    let defaults = "modelRoles:\n  default: ait-local-fixture/deepseek-v4.1-flash:max\n";
    std::fs::write(agent.join("config.yml"), defaults).unwrap();
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
    let mut spec = AgentSessionSpec {
        provider: "omp".to_owned(),
        cwd: root.path().to_str().unwrap().to_owned(),
        config: StoredAgentConfig::default(),
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    assert_eq!(details.features.len(), 2);
    for id in ["gpt-5.4", "deepseek-v4.1-flash"] {
        let route = json!(["ait-local-fixture", id]).to_string();
        let model = details
            .models
            .iter()
            .find(|model| model["id"] == route)
            .unwrap();
        assert!(
            model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|level| level["id"] == "max")
        );
        spec.config.model = Some(route);
        let features = client.draft_features(&spec).await.unwrap();
        assert_eq!(
            features.iter().any(|feature| feature["id"] == "fastMode"),
            id == "gpt-5.4"
        );
        assert_eq!(
            features.iter().any(|feature| feature["id"] == "slowMode"),
            id == "gpt-5.4"
        );
    }
    assert_eq!(
        std::fs::read_to_string(agent.join("config.yml")).unwrap(),
        defaults
    );
    assert_eq!(
        std::fs::read_to_string(agent.join("models.yml")).unwrap(),
        models.to_string()
    );
    assert!(!agent.join("sessions").exists());
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; audits ACP without inference or user configuration"]
async fn installed_omp_acp_opens_without_ait_specific_capabilities() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    std::fs::write(
        agent.join("models.yml"),
        json!({"providers":{"ait-local-fixture":{
            "baseUrl":"http://127.0.0.1:9/v1","api":"openai-completions","auth":"none",
            "models":[{"id":"fixture","name":"Local fixture","reasoning":false,
                "input":["text"],"contextWindow":32768,"maxTokens":1024,
                "cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0}}]}}})
        .to_string(),
    )
    .unwrap();
    let mut command = tokio::process::Command::new(program);
    command
        .args(["--mode", "acp", "--no-pty", "--no-session", "--session-dir"])
        .arg(root.path().join("sessions"))
        .args(["--provider", "ait-local-fixture", "--model", "fixture"])
        .env("PI_CODING_AGENT_DIR", &agent)
        .env("OMP_PROFILE", "")
        .env("PI_PROFILE", "");
    let mut transport = crate::local::acp_transport::Transport::spawn(
        command,
        root.path().to_str().unwrap(),
        Duration::from_secs(30),
        "Oh My Pi",
    )
    .unwrap();
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
                "clientCapabilities":{"elicitation":{"form":{}}},
                "clientInfo":{"name":"ait-compatibility-audit","version":"1"}}),
        )
        .await
        .unwrap();
    assert_eq!(initialized["protocolVersion"], 1);
    assert_eq!(initialized["agentCapabilities"]["loadSession"], true);
    let created = transport
        .request("session/new", json!({"cwd":root.path(),"mcpServers":[]}))
        .await
        .unwrap();
    let options = created["configOptions"].as_array().unwrap();
    for id in ["mode", "model", "thinking"] {
        assert!(options.iter().any(|option| option["id"] == id));
    }
    transport
        .request("session/close", json!({"sessionId":created["sessionId"]}))
        .await
        .unwrap();
    transport.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; uses isolated plugins and a local mock model"]
async fn installed_omp_plugins_use_generic_commands_ui_and_custom_message_history() {
    let fixture = installed_with_plugins().await;
    let client = &fixture.client;
    let spec = &fixture.spec;
    let commands = client.commands(spec).await.unwrap();
    for name in ["compat-vendor-alpha", "compat-other-command"] {
        assert!(
            commands.iter().any(|command| command["name"] == name),
            "missing {name}: {commands:?}"
        );
    }
    let mut session = client.create_session(spec).await.unwrap();
    for name in ["compat-vendor-alpha", "compat-other-command"] {
        session
            .start_turn(&format!("/{name}"), &spec.config)
            .await
            .unwrap();
        session.runtime_info().await.unwrap();
        let question = loop {
            if let AgentTurnEvent::PermissionRequested(request) =
                next(session.as_mut()).await.unwrap()
            {
                break request;
            }
        };
        session
            .respond_permission(
                question["id"].as_str().unwrap(),
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":"A, B"}}}),
            )
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["text"]==format!("{name}: A, B"))),"{events:?}");
    }
    session
        .start_turn(
            "Use the local test model to persist this conversation.",
            &spec.config,
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}: {:?}",
        session.failure_message()
    );
    assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry)
        if entry.item["type"]=="tool_call" && entry.item["name"]=="compat_dynamic_tool" && entry.item["status"]=="completed")),"{events:?}");
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();
    let history = client.history(&handle, &spec.cwd).await.unwrap();
    for name in ["compat-vendor-alpha", "compat-other-command"] {
        assert!(
            history
                .iter()
                .any(|entry| entry.item["text"] == format!("{name}: A, B")),
            "{history:?}"
        );
    }
}

struct Installed {
    _root: tempfile::TempDir,
    client: OmpClient,
    spec: AgentSessionSpec,
    _server: tokio_util::task::AbortOnDropHandle<()>,
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; answers native ask using a local mock model"]
async fn installed_omp_structured_ask_reaches_the_host_and_returns_answers_to_the_model() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("OMP_ASK_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();

    let request = loop {
        let event = next(session.as_mut()).await.unwrap();
        if let AgentTurnEvent::PermissionRequested(request) = event {
            break request;
        }
        assert!(
            !matches!(event, AgentTurnEvent::Failed | AgentTurnEvent::Completed(_)),
            "native ask ended without a question: {event:?}"
        );
    };
    assert_eq!(request["kind"], "question");
    assert!(request.get("description").is_none());
    assert_eq!(request["input"]["questions"][1]["multiSelect"], true);
    assert_eq!(session.pending_permissions(), vec![request.clone()]);
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow","updatedInput":{"answers":{
                "db":["A,B"],"features":["Auth","Search"]}}}),
        )
        .await
        .unwrap();

    let events = complete(session.as_mut()).await;
    assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    let Some(AgentTurnEvent::Completed(Some(reply))) = events.last() else {
        panic!("ask did not complete: {events:?}");
    };
    for answer in ["A,B", "Auth", "Search"] {
        assert!(
            reply.contains(answer),
            "model did not receive {answer}: {reply}"
        );
    }
    session.close().await.unwrap();
}

async fn installed_with_plugins() -> Installed {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    let extensions = agent.join("extensions");
    std::fs::create_dir_all(&extensions).unwrap();
    std::fs::write(
        extensions.join("generic-extension.ts"),
        include_str!("fixtures/generic-extension.ts"),
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(mock_plugin_completion),
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
    };
    Installed {
        _root: root,
        client,
        spec,
        _server: server,
    }
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; injects lost settlement around loopback-only inference"]
async fn installed_omp_settlement_uses_native_state_when_notification_is_lost() {
    let fixture = installed_with_plugins().await;
    settlement::verify(&fixture.client, &fixture.spec).await;
}

async fn mock_plugin_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let tool_finished = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|message| message["role"] == "tool");
    let asking = request["messages"].to_string().contains("OMP_ASK_FIXTURE");
    let writing = request["messages"]
        .to_string()
        .contains("OMP_WRITE_APPROVAL_FIXTURE");
    let executing = request["messages"]
        .to_string()
        .contains("OMP_EXEC_APPROVAL_FIXTURE");
    let delta = if tool_finished {
        let text = if asking || writing || executing {
            request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "tool")
                .map(|message| message["content"].to_string())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "Fixture complete".to_owned()
        };
        json!({"role":"assistant","content":text})
    } else if writing || executing {
        let (tool, arguments) = if writing {
            (
                "write",
                json!({"path":"approval-probe.html","content":"OMP_WRITE_OK"}),
            )
        } else {
            ("bash", json!({"command":"printf OMP_EXEC_OK"}))
        };
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture-approval","type":"function",
            "function":{"name":tool,"arguments":arguments.to_string()}}]})
    } else if asking {
        let fixture: Value =
            serde_json::from_str(include_str!("../../../../tests/fixtures/omp_question.json"))
                .unwrap();
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture-ask","type":"function",
            "function":{"name":"ask","arguments":json!({"questions":fixture["native"]["questions"]}).to_string()}}]})
    } else {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture-call","type":"function",
            "function":{"name":"compat_dynamic_tool","arguments":"{\"text\":\"plugin tool output\"}"}}]})
    };
    let first = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":"fixture","object":"chat.completion.chunk","created":0,"model":"fixture",
        "choices":[{"index":0,"delta":{},"finish_reason":if tool_finished {"stop"} else {"tool_calls"}}],
        "usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}});
    (
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
    )
}
