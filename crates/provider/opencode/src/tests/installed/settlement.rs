//! Isolated official ACP peer with a deliberately missing prompt response.
use domain::agent_runtime::{AgentRuntimeStatus, registry::AgentRuntimeRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;

use super::*;
use provider::service::agent_manager::AgentManager;
use provider::testing::Manager;

#[tokio::test]
#[ignore = "requires official AIT_TEST_OPENCODE_BIN; loopback-only inference and lost terminal injection"]
async fn installed_acp_missing_prompt_result_cancellation_exits_running_and_restores_history() {
    let mut fixture = installed(false).await;
    let marker = fixture.root.path().join("terminal-dropped");
    fixture.client.environment.insert(
        "AIT_TEST_OPENCODE_REAL_BIN".into(),
        fixture.client.program.to_str().unwrap().to_owned(),
    );
    fixture.client.environment.insert(
        "AIT_TEST_OPENCODE_DROP_MARKER".into(),
        marker.to_str().unwrap().to_owned(),
    );
    fixture.client.program = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/opencode_acp_proxy.py");
    fixture.client.deadline = Duration::from_secs(2);
    let registry = FileBackedAgentRuntimeRegistry::new(fixture.root.path().join("agents.json"));
    registry.initialize().unwrap();
    let timeline = provider::testing::memory_timeline().unwrap();
    let mut manager =
        Manager::new(AgentManager::new(Box::new(registry.clone())).with_timeline(timeline.clone()));
    manager.register(Box::new(fixture.client)).unwrap();
    manager.create("native", &fixture.spec).await.unwrap();
    manager
        .send("native", "Isolated lost prompt result")
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        while !marker.exists() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();

    tokio::time::timeout(Duration::from_millis(500), manager.cancel("native"))
        .await
        .expect("stop must not await a missing native result")
        .unwrap();
    poll_until_idle(&mut manager, "native").await;

    let record = registry.get("native").unwrap().unwrap();
    assert_eq!(record.last_status, AgentRuntimeStatus::Error);
    assert!(
        record
            .last_error
            .unwrap()
            .contains("did not confirm cancellation")
    );
    manager.send("native", "Continue").await.unwrap();
    poll_until_idle(&mut manager, "native").await;
    assert_eq!(
        registry.get("native").unwrap().unwrap().last_status,
        AgentRuntimeStatus::Idle
    );
    assert_eq!(
        provider::testing::timeline_entries(&timeline, "native")
            .unwrap()
            .1
            .iter()
            .filter(|row| row.item["type"] == "user_message")
            .count(),
        2
    );
    manager.close_all().await.unwrap();
}
