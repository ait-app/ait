use super::*;

#[tokio::test]
async fn inactive_controls_are_discoverable_and_unknown_actions_do_not_claim_a_writer() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry));
    manager.register_client(Box::new(client.clone())).unwrap();
    let record = manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    manager.close_all().await.unwrap();
    let before = std::fs::read(root.path().join("agent/calls.jsonl")).unwrap();
    let mut snapshot = json!({});
    manager.control_snapshot(&record, &mut snapshot);
    assert!(
        snapshot["providerUi"]["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|action| action["id"] == "navigate_tree")
    );
    assert!(
        manager
            .begin_action("agent", "switch_session", &json!({}))
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(root.path().join("agent/calls.jsonl")).unwrap(),
        before
    );
}

#[tokio::test]
async fn semantic_controls_keep_native_questions_interactive_without_a_foreground_turn() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    let mut receipt = session
        .begin_action("login", &json!({"providerId":"fixture"}))
        .await
        .unwrap();
    assert!(matches!(
        next(session.as_mut()).await.unwrap(),
        AgentTurnEvent::PermissionRequested(_)
    ));
    let ui = session.ui_state().unwrap();
    assert_eq!(ui["link"]["url"], "https://example.com/fixture-login");
    session
        .respond_permission(
            "control-question",
            &json!({"behavior":"allow","updatedInput":{"answers":{"value":"answer"}}}),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let _ = session.poll_turn().unwrap();
            tokio::select! {
                result = &mut receipt => break result.unwrap(),
                () = tokio::time::sleep(Duration::from_millis(2)) => {}
            }
        }
    })
    .await
    .unwrap();
    assert!(result["text"].as_str().unwrap().contains("answer"));
    assert!(session.ui_state().unwrap()["link"].is_null());
    assert!(
        session
            .begin_action("cancel_subagent", &json!({"subagentId":"foreign"}))
            .await
            .is_err()
    );
    session
        .start_turn("next prompt", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        complete(session.as_mut()).await.last(),
        Some(AgentTurnEvent::Completed(_))
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn exported_ui_survives_the_acp_bridge_and_clears_without_assistant_messages() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    session
        .start_turn("/fixture-ui", &spec.config)
        .await
        .unwrap();
    let events = complete(session.as_mut()).await;
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentTurnEvent::Timeline(_)))
    );
    let ui = session.ui_state().unwrap();
    assert_eq!(ui["statuses"][0]["text"], "Ready");
    assert_eq!(ui["widgets"][0]["lines"], json!(["Native widget"]));
    assert_eq!(ui["editor"]["text"], "Native draft");
    session
        .start_turn("/fixture-ui-clear", &spec.config)
        .await
        .unwrap();
    complete(session.as_mut()).await;
    assert_eq!(session.ui_state().unwrap()["statuses"], json!([]));
    assert_eq!(session.ui_state().unwrap()["widgets"], json!([]));
    session.close().await.unwrap();
    assert!(session.ui_state().is_none());
}

#[tokio::test]
async fn runtime_toggle_reaches_native_dispatch_and_only_finishes_after_configuration_commits() {
    let (_root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    for (action, feature) in [
        ("set_auto_compaction", "autoCompaction"),
        ("set_auto_retry", "autoRetry"),
    ] {
        for enabled in [false, true] {
            let mut receipt = session
                .begin_action(action, &json!({"enabled":enabled.to_string()}))
                .await
                .unwrap();
            let values = tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if let Some(AgentTurnEvent::FeatureValuesUpdated(values)) =
                        session.poll_turn().unwrap()
                    {
                        break values;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(values[feature], enabled);
            assert!(
                tokio::time::timeout(Duration::from_millis(1), &mut receipt)
                    .await
                    .is_err()
            );
            while session.poll_turn().unwrap().is_some() {}
            session.observation_committed();
            assert!(receipt.await.unwrap()["error"].is_null());
        }
    }
    session.close().await.unwrap();
}
