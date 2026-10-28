use super::*;

#[tokio::test]
async fn runtime_inspection_never_blocks_dialogs_before_prompt_admission() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("../../../../tests/fixtures/omp_dialogs.json")).unwrap();
    for case in fixtures {
        let native = &case["native"];
        let (root, client, spec) = fixture();
        let mut session = client.create_session(&spec).await.unwrap();
        std::fs::write(root.path().join("agent/dialog.json"), native.to_string()).unwrap();
        session
            .start_turn("interactive-fixture", &spec.config)
            .await
            .unwrap();

        let runtime = tokio::time::timeout(Duration::from_secs(2), session.runtime_info())
            .await
            .expect("runtime inspection blocked delivery of an OMP dialog")
            .unwrap();
        assert_eq!(runtime.provider, "omp");
        let request = loop {
            match next(session.as_mut()).await.unwrap() {
                AgentTurnEvent::PermissionRequested(request) => break request,
                AgentTurnEvent::Failed | AgentTurnEvent::Completed(_) => {
                    panic!("dialog ended without user input: {native}");
                }
                _ => {}
            }
        };
        assert_eq!(request, case["request"]);
        assert_eq!(session.pending_permissions(), vec![request.clone()]);
        session
            .respond_permission(request["id"].as_str().unwrap(), &case["response"])
            .await
            .unwrap();
        let events = complete(session.as_mut()).await;
        assert!(events.iter().any(|event| matches!(event,
            AgentTurnEvent::PermissionResolved(id) if *id == request["id"])));
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
        assert_eq!(session.pending_permissions(), Vec::<Value>::new());
        session.close().await.unwrap();
    }
}

#[tokio::test]
async fn manager_publishes_dialog_attention_and_reconnect_state_before_native_acknowledgement() {
    use crate::service::agent_manager::{AgentManager, AgentRegistration};
    use domain::agent_runtime::{AgentAttentionReason, registry::AgentRuntimeRegistry};
    use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
    let (root, client, spec) = fixture();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client)).unwrap();
    manager
        .create("agent", &spec, AgentRegistration::default())
        .await
        .unwrap();
    std::fs::write(
        root.path().join("agent/dialog.json"),
        json!({"method":"select","title":"Write approval","options":["Approve","Deny"]})
            .to_string(),
    )
    .unwrap();

    tokio::time::timeout(
        Duration::from_secs(2),
        manager.send("agent", "interactive-fixture"),
    )
    .await
    .expect("host submission blocked dialog polling")
    .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            manager.poll().await.unwrap();
            let record = registry.get("agent").unwrap().unwrap();
            if record.attention_reason == Some(AgentAttentionReason::Permission) {
                let mut snapshot = json!({});
                manager.control_snapshot(&record, &mut snapshot);
                assert_eq!(
                    snapshot["pendingPermissions"][0]["input"]["questions"][0]["question"],
                    "Write approval"
                );
                assert!(record.last_error.is_none());
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn steering_cannot_hide_a_dialog_while_native_admission_waits_for_its_answer() {
    let (root, client, spec) = fixture();
    let mut session = client.create_session(&spec).await.unwrap();
    std::fs::write(
        root.path().join("agent/dialog.json"),
        json!({"method":"input","title":"Additional instructions"}).to_string(),
    )
    .unwrap();
    let turn = session
        .start_turn("interactive-fixture", &spec.config)
        .await
        .unwrap();
    let request = loop {
        if let AgentTurnEvent::PermissionRequested(request) = next(session.as_mut()).await.unwrap()
        {
            break request;
        }
    };

    tokio::time::timeout(
        Duration::from_secs(2),
        session.steer_turn(&turn, "Continue after answering"),
    )
    .await
    .expect("steering blocked the pending dialog")
    .unwrap();
    assert_eq!(session.pending_permissions(), vec![request.clone()]);
    session
        .respond_permission(
            request["id"].as_str().unwrap(),
            &json!({"behavior":"allow",
        "updatedInput":{"answers":{"Additional instructions":"Answer"}}}),
        )
        .await
        .unwrap();

    let events = complete(session.as_mut()).await;
    assert!(
        matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
        "{events:?}"
    );
    session.close().await.unwrap();
}
