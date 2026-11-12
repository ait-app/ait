//! Agent MCP injection through ACP session opening of the offline `OpenCode` fake.

use super::*;
use crate::testing::agent_mcp::{Adapter, Opening, acp_opening, json_lines, verify};

const OPENINGS: [&str; 3] = ["session/new", "session/load", "session/resume"];

fn openings(root: &std::path::Path) -> Vec<Opening> {
    json_lines(&root.join("requests.jsonl"))
        .iter()
        .filter(|request| OPENINGS.contains(&request["method"].as_str().unwrap_or_default()))
        .map(|request| acp_opening(&request["params"]["mcpServers"]))
        .collect()
}

#[tokio::test]
async fn acp_openings_receive_private_mcp_and_rebind_after_restart() {
    let (root, client, spec) = fixture("normal");
    let build = || Box::new(client.clone()) as Box<dyn AgentClient>;
    let read = || openings(root.path());
    verify(&Adapter {
        client: &build,
        spec,
        openings: &read,
        prompts: true,
    })
    .await;
}

#[tokio::test]
async fn live_sessions_reject_mcp_changes_and_unadvertised_http() {
    let (_root, client, mut spec) = fixture("normal");
    spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private"}}),
    )]));
    let mut session = client.create_session(&spec).await.unwrap();
    let mut changed = spec.config.clone();
    changed.mcp_servers = None;
    assert_eq!(
        session.validate_config_update(&changed),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.start_turn("hello", &changed).await,
        Err(AgentSessionError::Rejected)
    );
    let handle = serde_json::to_string(&session.persistence()).unwrap();
    assert!(!handle.contains("Bearer private") && !handle.contains("7316"));
    session.close().await.unwrap();

    let (_root, client, _) = fixture("no-http-mcp");
    assert!(matches!(
        client.create_session(&spec).await,
        Err(AgentSessionError::Rejected)
    ));
}
