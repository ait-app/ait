#[tokio::test]
#[ignore = "requires AIT_TEST_MINIMAX_CODE_BIN and native login; performs model inference"]
async fn installed_latest_mcode_works_through_ait_and_restores_context() {
    let program =
        std::env::var_os("AIT_TEST_MINIMAX_CODE_BIN").expect("set AIT_TEST_MINIMAX_CODE_BIN");
    let fixture = crate::test_support::Fixture::new();
    super::execution::round_trip(&fixture, program.into(), true).await;
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
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::PermissionRequested(request)) => {
                    permissions += 1;
                    session
                        .respond_permission(
                            request["id"].as_str().unwrap(),
                            &json!({"behavior":"allow"}),
                        )
                        .await
                        .unwrap();
                }
                Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "tool_call" => {
                    tools += 1;
                }
                Some(AgentTurnEvent::Completed(_)) => break,
                Some(AgentTurnEvent::Failed) => {
                    panic!("native failure: {:?}", session.failure_message())
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
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
    session.cancel_turn(&turn).await.unwrap();
    let events = finish(session.as_mut()).await;
    assert_eq!(events.last(), Some(&AgentTurnEvent::Cancelled));
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
            .any(|option| { option["id"] == "low" })
    );
    let alternate = details
        .models
        .iter()
        .find(|model| {
            model["isDefault"] == false
                && model["id"]
                    .as_str()
                    .is_some_and(|id| id.ends_with(":thinking"))
        })
        .expect("latest mcode advertises an alternate thinking model");
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: cwd.into(),
        config: StoredAgentConfig {
            model: Some(model["id"].as_str().unwrap().into()),
            thinking_option_id: Some("low".into()),
            mode_id: Some("plan".into()),
            feature_values: Some(std::collections::BTreeMap::from([(
                "permissionMode".into(),
                json!("default"),
            )])),
            ..Default::default()
        },
    };
    let mut session = client.create_session(&spec).await.unwrap();
    let runtime = session.runtime_info().await.unwrap();
    assert_eq!(runtime.model, spec.config.model);
    assert_eq!(runtime.thinking_option_id.as_deref(), Some("low"));
    assert_eq!(runtime.mode_id.as_deref(), Some("plan"));
    assert_eq!(
        session.control_settings(&spec.config).unwrap()["features"][0]["value"],
        "default"
    );
    session.close().await.unwrap();

    let mut alternate_spec = spec;
    alternate_spec.config.model = Some(alternate["id"].as_str().unwrap().into());
    alternate_spec.config.thinking_option_id = None;
    let mut session = client.create_session(&alternate_spec).await.unwrap();
    assert_eq!(
        session.runtime_info().await.unwrap().model,
        alternate_spec.config.model
    );
    session.close().await.unwrap();
}
