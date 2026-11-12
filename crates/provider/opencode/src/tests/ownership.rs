use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use super::*;
use provider::service::agent_manager::AgentManager;
use provider::testing::{Lanes, Manager};

#[tokio::test]
async fn identical_configuration_keeps_distinct_native_sessions_in_separate_owner_lanes() {
    let (root, client, spec) = fixture("normal");
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let mut manager = Manager::new(AgentManager::new(Box::new(registry.clone())));
    manager.register(Box::new(client)).unwrap();
    let owners = Lanes::default();
    let mut first = manager.fork(&owners, "first");
    let mut second = manager.fork(&owners, "second");

    let created = first.create("agent-a", &spec).await.unwrap();
    let another = second.create("agent-b", &spec).await.unwrap();

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
