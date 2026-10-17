use super::*;

fn registration() -> AgentRegistration {
    AgentRegistration {
        internal: true,
        owner: Some(json!(domain::bot::SessionOwner {
            bot_id: domain::BotId::generate(),
            conversation_id: domain::ConversationId::generate(),
            workspace_id: "workspace-1".into(),
        })),
        ..AgentRegistration::default()
    }
}

#[tokio::test]
async fn bot_denies_every_native_request_without_publishing_approval_or_changing_config() {
    let (mut manager, registry, client) = make_manager();
    let connection = manager.events().connect();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let subscription = connection
        .subscribe(
            domain::session::protocol::EventsRequest {
                events: vec!["agent_permission_request".into()],
                notifications: false,
            },
            Arc::new(move |_, value| {
                captured.lock().unwrap().push(value);
                Ok(())
            }),
        )
        .unwrap();
    subscription.activate().unwrap();
    manager
        .create("agent-1", &spec(), registration())
        .await
        .unwrap();
    let original = registry.get("agent-1").unwrap().unwrap().config;
    manager.send("agent-1", "permission").await.unwrap();
    let kinds = ["tool", "question", "plan", "mode", "other"];
    client.0.lock().unwrap().events.extend(
        kinds.map(|kind| AgentTurnEvent::PermissionRequested(json!({"id":kind,"kind":kind}))),
    );
    manager.poll().await.unwrap();
    let responses = client.0.lock().unwrap().permission_responses.clone();
    assert_eq!(
        responses,
        kinds.map(|kind| (kind.into(), json!({"behavior":"deny"})))
    );
    assert!(events.lock().unwrap().is_empty());
    let record = registry.get("agent-1").unwrap().unwrap();
    assert!(!record.requires_attention);
    assert_eq!(record.config, original);
    assert!(manager.active_turn("agent-1").is_some());
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn bot_rejects_manual_grants_before_calling_the_native_provider() {
    let (mut manager, _, client) = make_manager();
    manager
        .create("agent-1", &spec(), registration())
        .await
        .unwrap();
    for response in [
        json!({"behavior":"allow"}),
        json!({"behavior":"deny","selectedActionId":"allow_always"}),
        json!({"behavior":"deny","updatedInput":{"permissions":["write"]}}),
    ] {
        assert_eq!(
            manager.permission("agent-1", "approval", &response).await,
            Err(ErrorCode::InvalidMessage)
        );
    }
    assert_eq!(
        client.0.lock().unwrap().permission_responses,
        [] as [(String, Value); 0]
    );
    manager.close_all().await.unwrap();
}

#[tokio::test]
async fn bot_stops_when_denial_fails_or_a_request_has_no_identity() {
    for error in [
        None,
        Some(AgentSessionError::Rejected),
        Some(AgentSessionError::Failed),
    ] {
        let (mut manager, registry, client) = make_manager();
        manager
            .create("agent-1", &spec(), registration())
            .await
            .unwrap();
        manager.send("agent-1", "permission").await.unwrap();
        {
            let mut state = client.0.lock().unwrap();
            state.permission_result = error.map_or(Ok(()), Err);
            state
                .events
                .push_back(AgentTurnEvent::PermissionRequested(if error.is_some() {
                    json!({"id":"approval"})
                } else {
                    json!({"kind":"tool"})
                }));
        }
        manager.poll().await.unwrap();
        assert_eq!(
            registry.get("agent-1").unwrap().unwrap().last_status,
            AgentRuntimeStatus::Error
        );
        assert!(manager.active_turn("agent-1").is_none());
        assert_eq!(client.0.lock().unwrap().close_calls, 1);
        manager.close_all().await.unwrap();
    }
}
