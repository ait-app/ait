use super::*;

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_COMMAND_BIN with native command lifecycle receipts; loopback mock model"]
async fn installed_custom_command_templates_and_arguments_execute_inside_native_acp() {
    let binary =
        std::env::var("AIT_TEST_OPENCODE_COMMAND_BIN").expect("set AIT_TEST_OPENCODE_COMMAND_BIN");
    let mut fixture = installed_with_binary(false, binary).await;
    let path = fixture.root.path().join("opencode.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config["command"] = json!({"ait-native-review":{"description":"Native review with arguments",
        "template":"AIT_NATIVE_COMMAND_SENTINEL: $ARGUMENTS"}});
    std::fs::write(path, config.to_string()).unwrap();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let catalog = session.refresh_commands().await.unwrap().unwrap();
    assert!(
        catalog
            .iter()
            .any(|command| command["name"] == "ait-native-review"
                && command["description"] == "Native review with arguments")
    );
    session
        .start_turn("/ait-native-review native-topic", &fixture.spec.config)
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let request = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let request = fixture.requests.recv().await.unwrap();
            if request["messages"]
                .to_string()
                .contains("AIT_NATIVE_COMMAND_SENTINEL: native-topic")
            {
                break request;
            }
        }
    })
    .await
    .unwrap();
    assert!(!request["messages"].to_string().contains("$ARGUMENTS"));
    session.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN=official 2.1.3; no custom command may be falsely completed"]
async fn installed_early_ack_commands_are_rejected_before_native_execution() {
    let mut fixture = installed(false).await;
    let path = fixture.root.path().join("opencode.json");
    let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    config["command"] =
        json!({"ait-native-review":{"template":"AIT_NATIVE_COMMAND_SENTINEL: $ARGUMENTS"}});
    std::fs::write(path, config.to_string()).unwrap();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    assert!(
        session
            .refresh_commands()
            .await
            .unwrap()
            .unwrap()
            .iter()
            .any(|command| command["name"] == "ait-native-review")
    );
    assert_eq!(
        session
            .start_turn("/ait-native-review native-topic", &fixture.spec.config)
            .await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        session
            .failure_message()
            .unwrap()
            .contains("lifecycle receipts")
    );
    session.close().await.unwrap();
    assert!(fixture.requests.try_recv().is_err());
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OPENCODE_BIN; isolated native ACP command catalog without inference"]
async fn installed_native_acp_commands_are_available_in_drafts_and_the_existing_session() {
    let fixture = installed(false).await;
    let catalog = fixture.client.commands(&fixture.spec).await.unwrap();
    assert!(catalog.iter().any(|command| command["name"] == "compact"));
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let live = session.refresh_commands().await.unwrap().unwrap();
    assert!(live.iter().any(|command| command["name"] == "compact"));
    assert_eq!(
        session.persistence().unwrap().session_id,
        session.runtime_info().await.unwrap().session_id.unwrap()
    );
    session.close().await.unwrap();
    assert!(fixture.requests.is_empty());
}
