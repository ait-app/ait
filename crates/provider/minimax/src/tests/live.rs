#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; validates advertised model creation"]
async fn installed_mcode_creates_sessions_for_every_selectable_model() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let client = MiniMaxClient::new(program.into());
    let details = client
        .discover(root.path().to_str().unwrap())
        .await
        .unwrap();
    let mut created = 0;
    for model in &details.models {
        if model["isSelectable"] == false {
            eprintln!("native rejected advertised model: {}", model["id"]);
            continue;
        }
        let spec = AgentSessionSpec {
            provider: PROVIDER.into(),
            cwd: root.path().to_str().unwrap().into(),
            config: StoredAgentConfig {
                model: Some(model["id"].as_str().unwrap().into()),
                thinking_option_id: model["defaultThinkingOptionId"].as_str().map(str::to_owned),
                ..StoredAgentConfig::default()
            },
            confinement: None,
        };
        let mut session = client.create_session(&spec).await.unwrap();
        let runtime = session.runtime_info().await.unwrap();
        assert_eq!(runtime.model, spec.config.model);
        assert_eq!(runtime.thinking_option_id, spec.config.thinking_option_id);
        session.close().await.unwrap();
        created += 1;
    }
    assert!(created > 0);
    eprintln!("created sessions for {created} selectable models");
}

#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; performs model inference"]
async fn installed_latest_mcode_works_through_ait_and_restores_context() {
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let fixture = provider::testing::Workbench::new();
    super::execution::round_trip(&fixture, program.into(), true).await;
}

async fn next_event(session: &mut dyn super::AgentSession) -> super::AgentTurnEvent {
    loop {
        if let Some(event) = session.poll_turn().unwrap() {
            return event;
        }
        tokio::time::sleep(super::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; exercises tools and cancellation"]
async fn installed_latest_mcode_tools_and_cancel() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let client =
        MiniMaxClient::new(program.into()).with_image_directory(root.path().join("images"));
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.path().to_str().unwrap().into(),
        config: StoredAgentConfig {
            feature_values: Some(std::collections::BTreeMap::from([(
                "permissionMode".into(),
                json!("default"),
            )])),
            ..Default::default()
        },
        confinement: None,
    };
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn(
            "Use your file writing tool to create ait-mcode-proof.txt in the current directory \
             containing exactly AIT_MCODE_TOOL_OK. Do not use subagents. Then reply DONE.",
            &spec.config,
        )
        .await
        .unwrap();
    let mut tools = 0;
    let mut permissions = 0;
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            match next_event(session.as_mut()).await {
                AgentTurnEvent::PermissionRequested(request) => {
                    permissions += 1;
                    session
                        .respond_permission(
                            request["id"].as_str().unwrap(),
                            &json!({"behavior":"allow"}),
                        )
                        .await
                        .unwrap();
                }
                AgentTurnEvent::Timeline(entry) if entry.item["type"] == "tool_call" => {
                    tools += 1;
                }
                AgentTurnEvent::Completed(_) => break,
                AgentTurnEvent::Failed => {
                    panic!("native failure: {:?}", session.failure_message())
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(tools > 0);
    assert_eq!(
        std::fs::read_to_string(root.path().join("ait-mcode-proof.txt"))
            .unwrap()
            .trim(),
        "AIT_MCODE_TOOL_OK"
    );
    eprintln!("native tools={tools}, permission callbacks={permissions}");
    let turn = session
        .start_turn(
            "Think carefully about all prime numbers below one million and explain each step.",
            &spec.config,
        )
        .await
        .unwrap();
    // Cancel a Turn that is streaming: native 0.7.7 answers before that Turn has ended.
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            if let AgentTurnEvent::Timeline(entry) = next_event(session.as_mut()).await
                && entry.item["type"] != "user_message"
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    session.cancel_turn(&turn).await.unwrap();
    let events = finish(session.as_mut()).await;
    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
    session
        .start_turn("Reply with exactly: AIT_MCODE_AFTER_CANCEL", &spec.config)
        .await
        .unwrap();
    let events = finish(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{:?}",
        session.failure_message()
    );
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; validates native controls"]
async fn installed_latest_mcode_confirms_model_thinking_mode_and_permission_choices() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let client = MiniMaxClient::new(program.into());
    let cwd = root.path().to_str().unwrap();
    let details = client.discover(cwd).await.unwrap();
    let model = details
        .models
        .iter()
        .find(|model| model["isDefault"] == true)
        .expect("latest mcode advertises the selected thinking model");
    assert!(
        model["thinkingOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| { option["id"] == "thinking:low" })
    );
    let alternate = details
        .models
        .iter()
        .find(|model| {
            model["isDefault"] == false
                && model["thinkingOptions"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(|option| option["id"] == "off"))
        })
        .expect("latest mcode advertises an alternate model with optional thinking");
    assert!(
        details
            .models
            .iter()
            .all(|model| !model["id"].as_str().unwrap().contains(":v:")),
        "reasoning variants are thinking options, not models"
    );
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: cwd.into(),
        config: StoredAgentConfig {
            model: Some(model["id"].as_str().unwrap().into()),
            thinking_option_id: Some("thinking:low".into()),
            mode_id: Some("plan".into()),
            feature_values: Some(std::collections::BTreeMap::from([(
                "permissionMode".into(),
                json!("default"),
            )])),
            ..Default::default()
        },
        confinement: None,
    };
    let mut session = client.create_session(&spec).await.unwrap();
    let runtime = session.runtime_info().await.unwrap();
    assert_eq!(runtime.model, spec.config.model);
    assert_eq!(runtime.thinking_option_id.as_deref(), Some("thinking:low"));
    assert_eq!(runtime.mode_id.as_deref(), Some("plan"));
    assert_eq!(
        session.control_settings(&spec.config).unwrap()["features"][0]["value"],
        "default"
    );
    session.close().await.unwrap();

    let mut alternate_spec = spec;
    alternate_spec.config.model = Some(alternate["id"].as_str().unwrap().into());
    for thinking in [None, Some("off")] {
        alternate_spec.config.thinking_option_id = thinking.map(str::to_owned);
        let mut session = client.create_session(&alternate_spec).await.unwrap();
        let runtime = session.runtime_info().await.unwrap();
        assert_eq!(runtime.model, alternate_spec.config.model);
        assert_eq!(
            runtime.thinking_option_id.as_deref(),
            Some(thinking.unwrap_or("thinking"))
        );
        session.close().await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; answers a native questionnaire"]
async fn installed_latest_mcode_asks_one_question_per_step_and_accepts_the_answer() {
    use super::*;
    let root = tempfile::tempdir().unwrap();
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let client = MiniMaxClient::new(program.into());
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.path().to_str().unwrap().into(),
        config: StoredAgentConfig::default(),
        confinement: None,
    };
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn(
            "Call your ask_user tool now with exactly one single-choice step whose header is \
             Language, asking whether I prefer Rust or Go. After I answer, reply with exactly \
             the option I chose and nothing else.",
            &spec.config,
        )
        .await
        .unwrap();
    let mut asked = false;
    let reply = tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            match next_event(session.as_mut()).await {
                AgentTurnEvent::PermissionRequested(request) => {
                    let questions = request["input"]["questions"].as_array().unwrap();
                    assert_eq!(questions.len(), 1, "{request}");
                    assert_eq!(questions[0]["header"], "Language", "{request}");
                    assert_eq!(questions[0]["allowOther"], true, "{request}");
                    assert!(request["metadata"]["toolCallId"].is_string(), "{request}");
                    let label = questions[0]["options"][0]["label"].clone();
                    session
                        .respond_permission(
                            request["id"].as_str().unwrap(),
                            &json!({"behavior":"allow","updatedInput":{
                                "answers":{"Language":label}}}),
                        )
                        .await
                        .unwrap();
                    asked = true;
                }
                AgentTurnEvent::Completed(reply) => break reply,
                AgentTurnEvent::Failed => {
                    panic!("native failure: {:?}", session.failure_message())
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(asked, "native ask_user did not reach the question request");
    eprintln!("native reply after the answer: {reply:?}");
    session.close().await.unwrap();
}
