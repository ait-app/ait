//! Agent MCP injection through the ACP openings of the offline AGY fake.

use super::*;
use provider::testing::agent_mcp::{Adapter, Opening, acp_opening, verify};

fn openings(directory: &TempDir) -> Vec<Opening> {
    logs(directory)
        .iter()
        .map(|entry| &entry["rpc"])
        .filter(|request| {
            matches!(
                request["method"].as_str(),
                Some("session/new" | "session/resume")
            )
        })
        .map(|request| acp_opening(&request["params"]["mcpServers"]))
        .collect()
}

#[tokio::test]
async fn acp_openings_receive_private_mcp_and_rebind_after_restart() {
    use std::os::unix::fs::PermissionsExt;

    let (directory, mut client, spec) = fixture();
    assert!(client.supports_agent_mcp());
    // The manager replaces client environment overrides with its launch environment, so the
    // fake's request log is set by an executable wrapper.
    let program = directory.path().join("agy-fixture");
    let fixture_program = serde_json::to_string(&client.program).unwrap();
    let log = serde_json::to_string(&directory.path().join("requests.jsonl")).unwrap();
    std::fs::write(
        &program,
        format!(
            "#!/usr/bin/env python3\nimport os,sys\nos.environ['AGY_FIXTURE_LOG']={log}\n\
             os.execv({fixture_program}, [{fixture_program}, *sys.argv[1:]])\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    client.program = program;
    let build = || Box::new(client.clone()) as Box<dyn AgentClient>;
    let read = || openings(&directory);
    verify(&Adapter {
        client: &build,
        spec,
        openings: &read,
        prompts: true,
    })
    .await;
}

#[tokio::test]
async fn changed_mcp_servers_reconnect_the_conversation_without_persisting_credentials() {
    let (directory, client, mut spec) = fixture();
    spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}}),
    )]));
    let mut session = client.create_session(&spec).await.unwrap();
    let created = session.persistence().unwrap();

    session
        .start_turn("hello", &StoredAgentConfig::default())
        .await
        .unwrap();
    completed(session.as_mut()).await;
    let reconnected = session.persistence().unwrap();
    assert_eq!(reconnected.session_id, created.session_id);
    for handle in [&created, &reconnected] {
        let handle = serde_json::to_string(handle).unwrap();
        assert!(!handle.contains("Bearer private") && !handle.contains("7316"));
    }
    session.close().await.unwrap();

    let resumed: Vec<Value> = logs(&directory)
        .into_iter()
        .map(|entry| entry["rpc"].clone())
        .filter(|request| request["method"] == "session/resume")
        .collect();
    assert_eq!(
        resumed.len(),
        1,
        "the conversation is resumed, not recreated"
    );
    assert_eq!(resumed[0]["params"]["sessionId"], created.session_id);
    let opened = openings(&directory);
    assert_eq!(opened.len(), 2, "the changed servers reopen the session");
    assert_eq!(
        opened[0]["ait"],
        json!({"url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}})
    );
    assert!(
        opened[1].is_empty(),
        "the reconnect carries the new servers"
    );
}
