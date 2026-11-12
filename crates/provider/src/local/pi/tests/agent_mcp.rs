//! Agent MCP injection through the private extension loaded by the offline Pi peer.

use super::*;
use crate::testing::agent_mcp::{Adapter, Opening, json_lines, verify};

fn openings(root: &std::path::Path) -> Vec<Opening> {
    json_lines(&root.join("mcp-openings.jsonl"))
        .into_iter()
        .map(|opening| serde_json::from_value(opening["servers"].clone()).unwrap())
        .collect()
}

#[tokio::test]
async fn extension_registers_private_mcp_and_rebinds_after_restart() {
    let (directory, client, spec) = fixture("");
    let build = || Box::new(client.clone()) as Box<dyn AgentClient>;
    let read = || openings(directory.path());
    verify(&Adapter {
        client: &build,
        spec,
        openings: &read,
        prompts: true,
    })
    .await;
}

#[tokio::test]
async fn credentials_are_owner_only_and_removed_with_the_native_process() {
    let (directory, client, mut spec) = fixture("");
    spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}}),
    )]));
    let mut session = client.create_session(&spec).await.unwrap();
    let opening = json_lines(&directory.path().join("mcp-openings.jsonl"))
        .pop()
        .unwrap();
    assert_eq!(opening["mode"], "0o600");
    let private = PathBuf::from(opening["directory"].as_str().unwrap());
    assert_eq!(
        std::fs::metadata(&private).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        opening["servers"]["ait"],
        json!({"url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}})
    );
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
    assert!(
        !serde_json::to_string(&session.persistence())
            .unwrap()
            .contains("Bearer private")
    );
    session.close().await.unwrap();
    assert!(
        !private.exists(),
        "the private extension is removed with Pi"
    );
}

#[test]
fn unsupported_mcp_transports_are_rejected() {
    for server in [
        json!({"type":"sse","url":"https://legacy.test/sse"}),
        json!({"type":"stdio","command":"relative-server"}),
    ] {
        let config = StoredAgentConfig {
            mcp_servers: Some(BTreeMap::from([("legacy".into(), server)])),
            ..StoredAgentConfig::default()
        };
        assert_eq!(config::validate(&config), Err(AgentSessionError::Rejected));
    }
    let config = StoredAgentConfig {
        mcp_servers: Some(BTreeMap::from([
            (
                "local".into(),
                json!({"type":"stdio","command":"/usr/bin/server","args":["--stdio"],
                    "env":{"MODE":"test"}}),
            ),
            (
                "literal".into(),
                json!({"type":"http","url":"https://docs.test/mcp",
                    "headers":{"X-Agent":"!agent $HOME ${PATH}"}}),
            ),
        ])),
        ..StoredAgentConfig::default()
    };
    assert_eq!(config::validate(&config), Ok(()));
    let extension = mcp::Extension::write(&config).unwrap().unwrap();
    let written: Value =
        serde_json::from_slice(&std::fs::read(extension.path().join("servers.json")).unwrap())
            .unwrap();
    assert_eq!(
        written,
        json!({"local":{"command":"/usr/bin/server","args":["--stdio"],"env":{"MODE":"test"}},
            "literal":{"url":"https://docs.test/mcp",
                "headers":{"X-Agent":"$!agent $$HOME $${PATH}"}}}),
        "Pi resolves `$` and a leading `!`; Ait values stay literal"
    );
    assert!(
        mcp::Extension::write(&StoredAgentConfig::default())
            .unwrap()
            .is_none()
    );
}
