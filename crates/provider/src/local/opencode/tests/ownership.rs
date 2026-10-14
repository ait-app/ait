use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use super::*;
use crate::service::agent_manager::{AgentManager, AgentRegistration, ownership::Owners};

#[tokio::test]
async fn identical_configuration_keeps_distinct_native_sessions_in_separate_owner_lanes() {
    let (root, client, spec) = fixture("normal");
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let mut manager = AgentManager::new(Box::new(registry.clone()));
    manager.register_client(Box::new(client)).unwrap();
    let owners = Owners::default();
    let mut first = manager.fork(Some(owners.owner("first".into())));
    let mut second = manager.fork(Some(owners.owner("second".into())));

    let created = first
        .create("agent-a", &spec, AgentRegistration::default())
        .await
        .unwrap();
    let another = second
        .create("agent-b", &spec, AgentRegistration::default())
        .await
        .unwrap();

    assert_ne!(
        created.persistence.unwrap().session_id,
        another.persistence.unwrap().session_id
    );
    assert_eq!(registry.list().unwrap().len(), 2);
    assert_eq!(owners.agent("agent-a").unwrap(), "first");
    assert_eq!(owners.agent("agent-b").unwrap(), "second");
    first.close_all().await.unwrap();
    second.close_all().await.unwrap();
}
