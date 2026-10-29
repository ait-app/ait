//! Host injection through the real ACP adapter, including next-turn settings and reopening.

use std::sync::Arc;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::daemon_config::FileDaemonConfigStore;

use super::*;
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::service::agent_mcp::AgentMcp;

fn manager(
    root: &std::path::Path,
    client: MiniMaxClient,
    port: u16,
    token: &str,
) -> (AgentManager, FileBackedAgentRuntimeRegistry) {
    let registry = FileBackedAgentRuntimeRegistry::new(root.join("agents.json"));
    registry.initialize().unwrap();
    let policy = Arc::new(FileDaemonConfigStore::with_defaults(
        root.join("config.json"),
    ));
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_agent_mcp(AgentMcp::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        token.to_owned().into(),
        policy,
    ));
    manager.register_client(Box::new(client)).unwrap();
    (manager, registry)
}

async fn turn(manager: &mut AgentManager, id: &str) {
    manager.send(id, "hello").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while manager.active_turn(id).is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(manager.last_message(id), Some("Hello from MiniMax"));
}

#[tokio::test]
async fn injected_mcp_survives_configuration_changes_and_rebinds_on_reopening() {
    let (root, client, spec) = fixture();
    let id = "minimax-agent";
    let token = uuid::Uuid::new_v4().to_string();
    let renewed = uuid::Uuid::new_v4().to_string();
    let (mut runtime, _) = manager(root.path(), client.clone(), 7316, &token);
    runtime
        .create(id, &spec, AgentRegistration::default())
        .await
        .unwrap();
    turn(&mut runtime, id).await;
    runtime
        .configure(
            id,
            &serde_json::from_value(json!({"modelId":"minimax/pro","modeId":"plan"})).unwrap(),
        )
        .await
        .unwrap();
    turn(&mut runtime, id).await;
    assert!(
        runtime
            .configure(
                id,
                &serde_json::from_value(json!({"mcpServers":{"custom":{
                    "type":"http","url":"https://example.test/mcp"}}}))
                .unwrap(),
            )
            .await
            .is_err(),
        "a live ACP session still rejects real MCP configuration changes"
    );
    runtime.close(id).await.unwrap();

    let (mut runtime, registry) = manager(root.path(), client, 8321, &renewed);
    turn(&mut runtime, id).await;
    runtime.close(id).await.unwrap();

    let requests: Vec<Value> = std::fs::read_to_string(root.path().join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let openings: Vec<_> = requests
        .iter()
        .filter(|request| {
            matches!(
                request["method"].as_str(),
                Some("session/new" | "session/resume")
            )
        })
        .collect();
    assert_eq!(
        openings.len(),
        2,
        "settings must not reopen the native session"
    );
    assert_eq!(openings[0]["method"], "session/new");
    assert_eq!(openings[1]["method"], "session/resume");
    for (opening, port, credential) in [(openings[0], 7316, &token), (openings[1], 8321, &renewed)]
    {
        let servers = opening["params"]["mcpServers"].as_array().unwrap();
        let internal = servers
            .iter()
            .find(|server| server["name"] == "ait")
            .expect("ACP receives the injected ait server");
        assert_eq!(
            internal["url"],
            format!("http://127.0.0.1:{port}/mcp/agents")
        );
        assert_eq!(internal["type"], "http");
        assert_eq!(
            internal["headers"],
            json!([
                {"name":"Authorization","value":format!("Bearer {credential}")},
                {"name":"X-Ait-Agent-Id","value":id}
            ])
        );
    }
    let record = registry.get(id).unwrap().unwrap();
    assert_eq!(
        record.config.as_ref().unwrap().model.as_deref(),
        Some("minimax/pro")
    );
    assert_eq!(
        record.config.as_ref().unwrap().mode_id.as_deref(),
        Some("plan")
    );
    assert!(record.config.as_ref().unwrap().mcp_servers.is_none());
    let durable = std::fs::read_to_string(root.path().join("agents.json")).unwrap();
    assert!(!durable.contains(&token));
    assert!(!durable.contains(&renewed));
}
