use super::*;

#[tokio::test]
async fn adapters_without_environment_support_reject_overrides_without_creating_a_session() {
    let (mut manager, registry, client) = make_manager();
    let environment = serde_json::from_value(json!({"KEY":"value"})).unwrap();
    assert_eq!(
        manager
            .create_with_environment(
                "agent-1",
                &spec(),
                AgentRegistration::default(),
                &environment
            )
            .await,
        Err(AgentManagerError::SessionRejected)
    );
    assert!(registry.list().unwrap().is_empty());
    assert_eq!(client.0.lock().unwrap().create_calls, 0);
}
