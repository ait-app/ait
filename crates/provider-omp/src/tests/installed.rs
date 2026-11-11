use super::*;

mod commands;
mod confinement;
mod images;
mod interactions;
mod settlement;
mod subagents;
mod timeline;

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
        confinement: None,
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
#[ignore = "requires AIT_TEST_OMP_BIN; runs only `omp --version`"]
async fn installed_omp_diagnostic_checks_the_version_without_a_session_process() {
    let root = tempfile::tempdir().unwrap();
    let program = std::env::var_os("AIT_TEST_OMP_BIN").expect("set AIT_TEST_OMP_BIN");
    let agent = root.path().join("agent");
    let client = |profile: &str| OmpClient {
        environment: AgentEnvironment::try_from(BTreeMap::from(
            [
                ("PI_CODING_AGENT_DIR", agent.to_str().unwrap()),
                ("OMP_PROFILE", profile),
                ("PI_PROFILE", ""),
                ("XDG_DATA_HOME", ""),
                ("XDG_STATE_HOME", ""),
                ("XDG_CACHE_HOME", ""),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned())),
        ))
        .unwrap(),
        ..OmpClient::new(program.clone().into())
    };

    let report = client("").diagnostic().await.unwrap();

    // OMP 18.18.8 prints `omp/<version>`.
    let version = report
        .lines()
        .find_map(|line| line.strip_prefix("  Version: omp/"))
        .unwrap_or_else(|| panic!("{report}"));
    assert!(
        report.contains(&format!(
            "\n  Version support: {version} (supported; minimum 18.12.6)"
        )),
        "{report}"
    );
    assert!(report.contains(&format!("\n  Agent directory: {}", agent.display())));
    assert!(report.contains("agent.db (not found)"), "{report}");
    assert!(!agent.exists());

    // OMP exits before printing its version for a profile it rejects.
    let report = client("bad/profile").diagnostic().await.unwrap();
    assert!(
        report.contains("\n  Version: unavailable (exited unsuccessfully)"),
        "{report}"
    );
    assert!(report.contains("invalid OMP_PROFILE"), "{report}");
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_BIN; reads real native model and Fast state without inference"]
async fn installed_omp_launch_flags_thinking_and_fast_follow_native_state_without_changing_defaults()
 {
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
        confinement: None,
    };
    let details = client.discover(&spec.cwd).await.unwrap();
    assert!(details.features.is_empty());
    for id in ["gpt-5.4", "deepseek-v4.1-flash"] {
        let route = json!(["ait-local-fixture", id]).to_string();
        let model = details
            .models
            .iter()
            .find(|model| model["id"] == route)
            .unwrap();
        assert_eq!(model["label"], id);
        assert_eq!(model["description"], format!("ait-local-fixture/{id}"));
        assert_eq!(
            model["thinkingOptions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|level| level["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["auto", "low", "high", "max"]
        );
        spec.config.model = Some(route);
        assert!(client.draft_features(&spec).await.unwrap().is_empty());
    }
    assert!(!agent.join("sessions").exists());

    spec.config.model = Some(json!(["ait-local-fixture", "gpt-5.4"]).to_string());
    spec.config.thinking_option_id = Some("high".to_owned());
    spec.config.feature_values = Some(BTreeMap::from([("fastMode".to_owned(), json!(true))]));
    let mut session = session::open(&client, &spec, None).await.unwrap();
    let fast = |session: &session::Session, config: &StoredAgentConfig| {
        session.control_settings(config).unwrap()["features"][0]["value"].clone()
    };
    for mode in ["full", "ask"] {
        let info = session.runtime_info().await.unwrap();
        assert_eq!(info.model, spec.config.model);
        assert_eq!(info.thinking_option_id.as_deref(), Some("high"));
        assert_eq!(info.mode_id.as_deref(), Some(mode));
        assert_eq!(fast(&session, &spec.config), true);
        // A mode change relaunches OMP; model, thinking and Fast carry across.
        spec.config.mode_id = Some("ask".to_owned());
        session.configure(&spec.config).await.unwrap();
    }
    session.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(agent.join("config.yml")).unwrap(),
        defaults
    );
    assert_eq!(
        std::fs::read_to_string(agent.join("models.yml")).unwrap(),
        models.to_string()
    );
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; resumes loopback-only history with other models"]
async fn installed_omp_resume_switches_only_to_a_listed_model() {
    let mut fixture = installed_with_plugins().await;
    let catalog = fixture.root.path().join("agent/models.yml");
    let mut models: Value =
        serde_json::from_str(&std::fs::read_to_string(&catalog).unwrap()).unwrap();
    let entries = models["providers"]["ait-local-fixture"]["models"]
        .as_array_mut()
        .unwrap();
    let mut second = entries[0].clone();
    second["id"] = json!("second");
    entries.push(second);
    std::fs::write(&catalog, models.to_string()).unwrap();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn(
            "Use the local test model to persist this conversation.",
            &fixture.spec.config,
        )
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    // A listed model replaces the session's; an unlisted one leaves OMP's choice in place.
    for (requested, expected) in [("second", "second"), ("removed", "second")] {
        fixture.spec.config.model = Some(json!(["ait-local-fixture", requested]).to_string());
        let mut resumed = fixture
            .client
            .resume_session(&handle, &fixture.spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        let info = resumed.runtime_info().await.unwrap();
        assert_eq!(info.session_id.as_deref(), Some(handle.session_id.as_str()));
        assert_eq!(
            info.model,
            Some(json!(["ait-local-fixture", expected]).to_string())
        );
        resumed.close().await.unwrap();
    }
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
        assert!(events.iter().any(|event|matches!(event,AgentTurnEvent::Timeline(entry) if entry.item["detail"]["text"]==format!("{name}: A, B"))),"{events:?}");
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
                .any(|entry| entry.item["detail"]["text"] == format!("{name}: A, B")),
            "{history:?}"
        );
    }
}

struct Installed {
    root: tempfile::TempDir,
    client: OmpClient,
    spec: AgentSessionSpec,
    _server: tokio_util::task::AbortOnDropHandle<()>,
}

/// OMP 18.18.8's `ask` takes one HTML form: a radio group and an optional checkbox group.
const ASK_FORM: &str = "<form><fieldset><legend>Which database?</legend>\
    <label><input type=\"radio\" name=\"db\" value=\"A,B\"> A,B</label>\
    <label><input type=\"radio\" name=\"db\" value=\"C\"> C</label></fieldset>\
    <fieldset><legend>Which features?</legend>\
    <label><input type=\"checkbox\" name=\"features\" value=\"Auth\"> Auth</label>\
    <label><input type=\"checkbox\" name=\"features\" value=\"Search\"> Search</label>\
    </fieldset></form>";

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; answers native ask using a local mock model"]
async fn installed_omp_structured_ask_reaches_the_host_and_returns_answers_to_the_model() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    session
        .start_turn("OMP_ASK_FIXTURE", &fixture.spec.config)
        .await
        .unwrap();

    // OMP asks one field at a time; each choice field is one card.
    let mut answered = Vec::new();
    for (title, multi, answer) in [
        ("Which database?", false, json!("A,B")),
        (
            "Which features?",
            true,
            json!(["Auth", "Search", "Custom feature"]),
        ),
    ] {
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
        let question = &request["input"]["questions"][0];
        assert_eq!(request["kind"], "question");
        assert_eq!(question["question"], title);
        assert_eq!(question["multiSelect"], multi);
        assert_eq!(question["allowOther"], true);
        assert_eq!(session.pending_permissions(), vec![request.clone()]);
        session
            .respond_permission(
                request["id"].as_str().unwrap(),
                &json!({"behavior":"allow","updatedInput":{"answers":{"value":answer}}}),
            )
            .await
            .unwrap();
        answered.push(request);
    }

    let events = complete(session.as_mut()).await;
    assert_eq!(session.pending_permissions(), Vec::<Value>::new());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::PermissionRequested(_))),
        "{events:?}"
    );
    let Some(AgentTurnEvent::Completed(Some(reply))) = events.last() else {
        panic!("ask did not complete: {events:?}");
    };
    for answer in ["A,B", "Auth", "Search", "Custom feature"] {
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
        confinement: None,
    };
    Installed {
        root,
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

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs a builtin command without inference"]
async fn installed_omp_local_commands_settle_with_an_echo_and_their_output() {
    let fixture = installed_with_plugins().await;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let prompt = AgentPrompt {
        client_message_id: Some("client-1".to_owned()),
        ..AgentPrompt::text("/session")
    };
    session
        .start_input(&prompt, &fixture.spec.config)
        .await
        .unwrap();

    let events = complete(session.as_mut()).await;

    let Some(AgentTurnEvent::Completed(Some(output))) = events.last() else {
        panic!("/session did not complete: {events:?}");
    };
    assert!(output.starts_with("Session: "), "{output}");
    let items: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AgentTurnEvent::Timeline(entry) => Some(&entry.item),
            _ => None,
        })
        .collect();
    assert_eq!(items[0]["type"], "user_message", "{events:?}");
    assert_eq!(items[0]["text"], "/session");
    assert_eq!(items[0]["clientMessageId"], "client-1");
    assert_eq!(items[1]["text"], output.as_str());
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; aborts a stalled loopback-only request"]
async fn installed_omp_abort_runs_queued_steering_as_its_own_turn() {
    let fixture = installed_with_plugins().await;
    let config = &fixture.spec.config;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let cancelled = session
        .start_turn("OMP_STALL_FIXTURE: wait for the stalled model.", config)
        .await
        .unwrap();
    loop {
        if let AgentTurnEvent::Timeline(entry) = next(session.as_mut()).await.unwrap()
            && entry.item["type"] == "user_message"
        {
            break;
        }
    }
    let steer = AgentPrompt {
        client_message_id: Some("steer-1".to_owned()),
        ..AgentPrompt::text("Steered while the model stalls.")
    };
    session.steer_input(&cancelled, &steer).await.unwrap();
    // OMP drops steering it has not dequeued when `abort` arrives, so let it queue first.
    for _ in 0..50 {
        assert!(session.poll_turn().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    session.cancel_turn(&cancelled).await.unwrap();

    let events = complete(session.as_mut()).await;
    assert_eq!(
        events.last(),
        Some(&AgentTurnEvent::Cancelled),
        "{events:?}"
    );
    let AgentTurnEvent::Started(queued) = next(session.as_mut()).await.unwrap() else {
        panic!("OMP's queued steering did not start a turn");
    };
    assert_ne!(queued, cancelled);
    let events = complete(session.as_mut()).await;

    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}: {:?}",
        session.failure_message()
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(entry)
            if entry.item["type"] == "user_message"
                && entry.item["clientMessageId"] == "steer-1")),
        "{events:?}"
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires official AIT_TEST_OMP_BIN; runs loopback-only inference"]
async fn installed_omp_user_messages_carry_native_entry_ids_and_skill_prompts_echo() {
    let fixture = installed_with_plugins().await;
    let skill = fixture.root.path().join(".claude/skills/review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nReview the code carefully.\n",
    )
    .unwrap();
    let config = &fixture.spec.config;
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let mut live = Vec::new();
    for (text, client) in [("Name me by my entry.", "c1"), ("/skill:review x", "c2")] {
        let prompt = AgentPrompt {
            client_message_id: Some(client.to_owned()),
            ..AgentPrompt::text(text)
        };
        session.start_input(&prompt, config).await.unwrap();
        live.extend(user_messages(&complete(session.as_mut()).await));
    }
    let handle = session.persistence().unwrap();
    session.close().await.unwrap();

    // OMP 18.18.8 announces neither with an entry id; Ait asks get_branch_messages (D9).
    assert_eq!(live.len(), 2, "{live:?}");
    assert_eq!(
        live[0].item["messageId"],
        json!(recorded_user_ids(&handle)[0])
    );
    assert_eq!(live[0].item["clientMessageId"], "c1");
    // The bubble is OMP's skill expansion, not a literal user message.
    assert!(live[1].key.contains(":custom:"), "{live:?}");
    assert_eq!(live[1].item["text"], "/skill:review x");
    assert_eq!(live[1].item["clientMessageId"], "c2");
    let history = fixture
        .client
        .history(&handle, &fixture.spec.cwd)
        .await
        .unwrap();
    let replayed: Vec<_> = history
        .into_iter()
        .filter(|entry| entry.item["type"] == "user_message")
        .collect();
    assert_eq!(replayed, live);
}

async fn mock_plugin_completion(
    axum::Json(request): axum::Json<Value>,
) -> ([(&'static str, &'static str); 1], String) {
    let stalled = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .is_some_and(|message| message["content"].to_string().contains("OMP_STALL_FIXTURE"));
    if stalled {
        // Hold the request open until OMP aborts it.
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
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
        let arguments = json!({"i":"Pick a stack","html":ASK_FORM});
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"fixture-ask","type":"function",
            "function":{"name":"ask","arguments":arguments.to_string()}}]})
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
