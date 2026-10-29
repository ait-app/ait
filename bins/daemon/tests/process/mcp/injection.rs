//! Host-to-native-client MCP discovery, invocation and daemon restart integration.

use super::*;

fn launches(cwd: &std::path::Path, id: &str) -> Vec<Value> {
    std::fs::read_to_string(cwd.join("native-requests.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|request| {
            request["params"]["config"]["mcp_servers"]["ait"]["http_headers"]["X-Ait-Agent-Id"]
                == id
        })
        .collect()
}

async fn assert_child(client: &Client, cwd: &std::path::Path, id: &str, workspace: &str) {
    let child: Value = serde_json::from_str(
        &std::fs::read_to_string(cwd.join("mcp-injection-child.json")).unwrap(),
    )
    .unwrap();
    let child_id = child["agentId"].as_str().unwrap();
    assert_ne!(child_id, id);
    assert_eq!(child["agent"]["workspaceId"], workspace);
    assert_eq!(
        client
            .tool("get_agent_status", json!({"agentId":child_id}), None)
            .await["snapshot"]["labels"]["paseo.parent-agent-id"],
        id
    );
    assert_eq!(launches(cwd, child_id).len(), 1);
}

fn assert_private(state: &std::path::Path, log: &std::path::Path, credential: &str) {
    for path in [
        state.join("agents/agents.json"),
        state.join("config.json"),
        log.to_path_buf(),
    ] {
        assert!(!std::fs::read_to_string(path).unwrap().contains(credential));
    }
}

async fn probe(client: &Client, id: &str) {
    client
        .tool(
            "send_agent_prompt",
            json!({"agentId":id,"prompt":"ait-mcp-probe"}),
            None,
        )
        .await;
    assert_eq!(
        client
            .tool("wait_for_agent", json!({"agentId":id}), None)
            .await["lastMessage"],
        "Echo: mcp-injection-ok"
    );
}

#[tokio::test]
async fn native_client_calls_injected_mcp_across_turns_and_daemon_restart() {
    let NativeFixture { root, cwd, path } = NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let client = Client::new(&address);
    let workspace = client
        .tool(
            "create_workspace",
            json!({"path":cwd,"isolation":"local"}),
            None,
        )
        .await;
    let workspace_id = workspace["workspace"]["id"].as_str().unwrap();
    let created = client
        .tool(
            "create_agent",
            json!({"provider":"codex/offline-model","workspaceId":workspace_id,
                "title":"MCP injection probe","initialPrompt":"ready"}),
            None,
        )
        .await;
    let id = created["agentId"].as_str().unwrap();
    client
        .tool("wait_for_agent", json!({"agentId":id}), None)
        .await;

    probe(&client, id).await;
    probe(&client, id).await;

    assert_child(&client, &cwd, id, workspace_id).await;
    let before = launches(&cwd, id);
    assert_eq!(
        before.len(),
        1,
        "later turns must keep the original native MCP connection"
    );
    let server = &before[0]["params"]["config"]["mcp_servers"]["ait"];
    assert_eq!(server["url"], format!("http://{address}/mcp/agents"));
    assert_eq!(server["http_headers"]["X-Ait-Agent-Id"], id);
    let authorization = server["http_headers"]["Authorization"].as_str().unwrap();
    assert_ne!(authorization, format!("Bearer {TOKEN}"));
    assert_private(&state, &log, authorization.trim_start_matches("Bearer "));
    let listed = client.tool("list_workspaces", json!({}), None).await;
    assert_eq!(
        listed["workspaces"][0]["title"],
        "Renamed through injected MCP"
    );
    terminate(&mut process).await;

    let mut process = start_with_path(&state, &log, Some(&path));
    let resumed_address = ready(&mut process, &log).await;
    let resumed = Client::new(&resumed_address);

    assert_eq!(
        resumed
            .http
            .post(&resumed.url)
            .header("authorization", authorization)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    probe(&resumed, id).await;

    let after = launches(&cwd, id);
    let server = &after.last().unwrap()["params"]["config"]["mcp_servers"]["ait"];
    assert_eq!(
        server["url"],
        format!("http://{resumed_address}/mcp/agents")
    );
    assert_eq!(server["http_headers"]["X-Ait-Agent-Id"], id);
    let renewed = server["http_headers"]["Authorization"].as_str().unwrap();
    assert_ne!(
        renewed, authorization,
        "the private MCP credential rotates on daemon restart"
    );
    assert_private(&state, &log, renewed.trim_start_matches("Bearer "));
    let probes: Vec<Value> = std::fs::read_to_string(cwd.join("mcp-injection-probes.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(probes.len(), 3);
    assert!(probes.iter().all(|probe| probe["caller"] == id));
    terminate(&mut process).await;
}
