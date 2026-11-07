use super::*;

fn automation() -> AgentRegistration {
    AgentRegistration {
        labels: [(
            model::automation::ORIGIN_LABEL.to_owned(),
            model::automation::ORIGIN.to_owned(),
        )]
        .into(),
        ..AgentRegistration::default()
    }
}

#[tokio::test]
async fn automation_agents_stay_visible_but_deny_requests_and_manual_grants() {
    let (mut manager, registry, client) = make_manager();
    manager
        .create("agent-1", &spec(), automation())
        .await
        .unwrap();
    assert!(!registry.get("agent-1").unwrap().unwrap().internal);
    manager.send("agent-1", "permission").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::PermissionRequested(
            json!({"id":"tool","kind":"tool"}),
        ));
    manager.poll().await.unwrap();
    assert_eq!(
        client.0.lock().unwrap().permission_responses,
        [("tool".to_owned(), json!({"behavior":"deny"}))]
    );
    assert!(!registry.get("agent-1").unwrap().unwrap().requires_attention);
    assert_eq!(
        manager
            .permission("agent-1", "tool", &json!({"behavior":"allow"}))
            .await,
        Err(ErrorCode::InvalidMessage)
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn other_labels_keep_interactive_approval() {
    let (mut manager, registry, client) = make_manager();
    let registration = AgentRegistration {
        labels: [(
            model::automation::ORIGIN_LABEL.to_owned(),
            "user".to_owned(),
        )]
        .into(),
        ..AgentRegistration::default()
    };
    manager
        .create("agent-1", &spec(), registration)
        .await
        .unwrap();
    manager.send("agent-1", "permission").await.unwrap();
    client
        .0
        .lock()
        .unwrap()
        .events
        .push_back(AgentTurnEvent::PermissionRequested(
            json!({"id":"tool","kind":"tool"}),
        ));
    manager.poll().await.unwrap();
    assert!(client.0.lock().unwrap().permission_responses.is_empty());
    assert!(registry.get("agent-1").unwrap().unwrap().requires_attention);
    manager.close_all().await.unwrap();
}
