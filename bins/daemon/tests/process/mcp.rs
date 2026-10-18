use serde_json::{Value, json};

use super::native::NativeFixture;
use super::{TOKEN, ready, start_with_path, terminate};

struct Client {
    http: reqwest::Client,
    url: String,
}

impl Client {
    fn new(address: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(70))
                .build()
                .unwrap(),
            url: format!("http://{address}/mcp/agents"),
        }
    }

    async fn rpc(&self, method: &str, params: Value, caller: Option<&str>) -> Value {
        let mut request = self
            .http
            .post(&self.url)
            .bearer_auth(TOKEN)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":"same-request","method":method,"params":params}));
        if let Some(caller) = caller {
            request = request.header("x-ait-agent-id", caller);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        response.json().await.unwrap()
    }

    async fn tool(&self, name: &str, arguments: Value, caller: Option<&str>) -> Value {
        let response = self
            .rpc(
                "tools/call",
                json!({"name":name,"arguments":arguments}),
                caller,
            )
            .await;
        assert!(response.get("error").is_none(), "{name}: {response}");
        assert_eq!(response["result"]["isError"], false, "{name}: {response}");
        response["result"]["structuredContent"].clone()
    }
}

#[tokio::test]
async fn mcp_catalog_configuration_and_workspace_lifecycle() {
    let NativeFixture { root, cwd, path } = NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let client = Client::new(&address);
    let list = client.rpc("tools/list", json!({}), None).await;
    assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 40);
    let workspace = client.tool("create_workspace",json!({"isolation":"local","path":cwd,"title":"MCP workspace","idempotencyKey":"mcp-workspace"}),None).await;
    let id = workspace["workspace"]["id"]
        .as_str()
        .or_else(|| workspace["workspace"]["workspaceId"].as_str())
        .unwrap();
    let replay = client.tool("create_workspace",json!({"isolation":"local","path":cwd,"title":"MCP workspace","idempotencyKey":"mcp-workspace"}),None).await;
    assert_eq!(replay["workspace"], workspace["workspace"]);
    assert_eq!(
        client
            .tool(
                "rename_workspace",
                json!({"workspaceId":id,"title":"Renamed"}),
                None
            )
            .await["accepted"],
        true
    );
    assert_eq!(
        client.tool("list_workspaces", json!({}), None).await["workspaces"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        client.tool("list_profiles", json!({}), None).await["profiles"],
        json!([])
    );
    let invalid = client.rpc("tools/call",json!({"name":"create_workspace","arguments":{"isolation":"local","path":cwd,"unexpected":true}}),None).await;
    assert_eq!(invalid["error"]["code"], -32602);
    let missing = client.rpc("tools/call",json!({"name":"rename_workspace","arguments":{"workspaceId":"wks_missing","title":"x"}}),None).await;
    assert_eq!(missing["result"]["isError"], true);
    client
        .tool("archive_workspace", json!({"workspaceId":id}), None)
        .await;
    assert!(
        client.tool("list_workspaces", json!({}), None).await["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut socket = super::transport::connect(&address, &["daemon.config.set.request"]).await;
    super::transport::request(
        &mut socket,
        "daemon.config.set.request",
        json!({"config":{"mcp":{"enabled":false,"injectIntoAgents":false}}}),
    )
    .await;
    assert_eq!(
        client
            .http
            .post(&client.url)
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    super::transport::request(
        &mut socket,
        "daemon.config.set.request",
        json!({"config":{"mcp":{"enabled":true,"injectIntoAgents":false}}}),
    )
    .await;
    assert_eq!(
        client.rpc("tools/list", json!({}), None).await["result"]["tools"]
            .as_array()
            .unwrap()
            .len(),
        40
    );
    terminate(&mut process).await;
    assert!(!std::fs::read_to_string(&log).unwrap().contains(TOKEN));
}

#[tokio::test]
async fn mcp_native_agent_calls_preserve_parentage_and_input_admission() {
    let NativeFixture { root, cwd, path } = NativeFixture::new();
    let log = root.path().join("server.log");
    let mut process = start_with_path(&root.path().join("state"), &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let client = Client::new(&address);
    assert_eq!(
        client.tool("list_providers", json!({}), None).await["providers"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert!(
        !client
            .tool("list_models", json!({"provider":"codex","cwd":cwd}), None)
            .await["models"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client
        .tool(
            "inspect_provider",
            json!({"provider":"codex","cwd":cwd}),
            None,
        )
        .await;
    let input = json!({"provider":"codex/offline-model","cwd":cwd,"title":"MCP root","initialPrompt":"hello MCP","idempotencyKey":"mcp-agent"});
    let created = client.tool("create_agent", input.clone(), None).await;
    let id = created["agentId"].as_str().unwrap();
    assert_eq!(
        client.tool("create_agent", input, None).await["agentId"],
        id
    );
    assert_eq!(
        client
            .tool("wait_for_agent", json!({"agentId":id}), None)
            .await["lastMessage"],
        "Echo: hello MCP"
    );
    let status = client
        .tool("get_agent_status", json!({"agentId":id}), None)
        .await;
    assert_eq!(status["status"], "idle");
    assert!(
        client
            .tool("get_agent_activity", json!({"agentId":id,"limit":10}), None)
            .await["content"]
            .as_str()
            .unwrap()
            .contains("hello MCP")
    );
    exercise_child(&client, id, &status).await;
    terminate(&mut process).await;
}

async fn exercise_child(client: &Client, id: &str, status: &Value) {
    let child = client
        .tool(
            "create_agent",
            json!({"provider":"codex/offline-model","title":"MCP child","initialPrompt":"child task"}),
            Some(id),
        )
        .await;
    let child_id = child["agentId"].as_str().unwrap();
    assert_eq!(
        child["agent"]["workspaceId"],
        status["snapshot"]["workspaceId"]
    );
    let child_status = client
        .tool("get_agent_status", json!({"agentId":child_id}), None)
        .await;
    assert_eq!(
        child_status["snapshot"]["labels"]["paseo.parent-agent-id"],
        id
    );
    client
        .tool("wait_for_agent", json!({"agentId":child_id}), None)
        .await;
    client.tool("update_agent",json!({"agentId":child_id,"name":"Updated child","settings":{"thinkingOptionId":"high"}}),None).await;
    client
        .tool(
            "set_agent_mode",
            json!({"agentId":child_id,"modeId":"full-access"}),
            None,
        )
        .await;
    client
        .tool(
            "send_agent_prompt",
            json!({"agentId":child_id,"prompt":"next task"}),
            None,
        )
        .await;
    assert_eq!(
        client
            .tool("wait_for_agent", json!({"agentId":child_id}), None)
            .await["lastMessage"],
        "Echo: next task"
    );
    assert_eq!(
        client.tool("list_agents", json!({"limit":1}), None).await["agents"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        client
            .tool("list_pending_permissions", json!({}), None)
            .await["permissions"],
        json!([])
    );
    exercise_permissions(client, child_id).await;
    client
        .tool(
            "send_agent_prompt",
            json!({"agentId":child_id,"prompt":"hang"}),
            None,
        )
        .await;
    let timed_out = client
        .tool(
            "wait_for_agent",
            json!({"agentId":child_id,"timeoutMs":30}),
            None,
        )
        .await;
    assert_eq!(timed_out["status"], "timeout");
    assert_eq!(
        client
            .tool("get_agent_status", json!({"agentId":child_id}), None)
            .await["status"],
        "running"
    );
    client
        .tool("cancel_agent", json!({"agentId":child_id}), None)
        .await;
    client
        .tool("wait_for_agent", json!({"agentId":child_id}), None)
        .await;
    client
        .tool("archive_agent", json!({"agentId":child_id}), None)
        .await;
    client.tool("kill_agent", json!({"agentId":id}), None).await;
}

async fn exercise_permissions(client: &Client, agent_id: &str) {
    client
        .tool(
            "send_agent_prompt",
            json!({"agentId":agent_id,"prompt":"permit-command"}),
            None,
        )
        .await;
    let waited = client
        .tool("wait_for_agent", json!({"agentId":agent_id}), None)
        .await;
    assert_eq!(waited["status"], "permission");
    let pending = client
        .tool("list_pending_permissions", json!({}), None)
        .await;
    let permission = pending["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|permission| permission["agentId"] == agent_id)
        .unwrap();
    client.tool("respond_to_permission", json!({"agentId":agent_id,"requestId":permission["request"]["id"],"response":{"behavior":"allow"}}), None).await;
    assert_eq!(
        client
            .tool("wait_for_agent", json!({"agentId":agent_id}), None)
            .await["status"],
        "idle"
    );
    assert!(
        client
            .tool("list_pending_permissions", json!({}), None)
            .await["permissions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn mcp_managed_terminals_scripts_schedules_and_owned_heartbeats() {
    let NativeFixture { root, cwd, path } = NativeFixture::new();
    std::fs::write(
        cwd.join("ait.json"),
        r#"{"scripts":{"service":{"type":"service","command":"while :; do sleep 1; done"}}}"#,
    )
    .unwrap();
    let log = root.path().join("server.log");
    let mut process = start_with_path(&root.path().join("state"), &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let client = Client::new(&address);
    let created = client.tool("create_agent",json!({"provider":"codex/offline-model","cwd":cwd,"title":"Scheduler","initialPrompt":"ready"}),None).await;
    let agent_id = created["agentId"].as_str().unwrap();
    client
        .tool("wait_for_agent", json!({"agentId":agent_id}), None)
        .await;
    let workspace_id = created["agent"]["workspaceId"].as_str().unwrap();
    exercise_terminals(&client, agent_id, workspace_id).await;
    exercise_schedules(&client, agent_id, workspace_id).await;
    terminate(&mut process).await;
}

async fn exercise_terminals(client: &Client, agent_id: &str, workspace_id: &str) {
    assert_eq!(
        client
            .tool(
                "list_workspace_scripts",
                json!({"workspaceId":workspace_id}),
                None
            )
            .await["scripts"][0]["scriptName"],
        "service"
    );
    client
        .tool(
            "start_workspace_script",
            json!({"workspaceId":workspace_id,"scriptName":"service"}),
            None,
        )
        .await;
    client
        .tool(
            "stop_workspace_script",
            json!({"workspaceId":workspace_id,"scriptName":"service"}),
            None,
        )
        .await;
    let terminal = client
        .tool(
            "create_terminal",
            json!({"command":"/bin/cat","name":"MCP PTY"}),
            Some(agent_id),
        )
        .await;
    let terminal_id = terminal["terminal"]["id"].as_str().unwrap();
    client
        .tool(
            "send_terminal_keys",
            json!({"terminalId":terminal_id,"keys":"mcp-terminal-marker\n","literal":true}),
            None,
        )
        .await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        let captured = client
            .tool(
                "capture_terminal",
                json!({"terminalId":terminal_id,"scrollback":true}),
                None,
            )
            .await;
        if captured["lines"]
            .to_string()
            .contains("mcp-terminal-marker")
        {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        !client
            .tool("list_terminals", json!({"workspaceId":workspace_id}), None)
            .await["terminals"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    client
        .tool("kill_terminal", json!({"terminalId":terminal_id}), None)
        .await;
}

async fn exercise_schedules(client: &Client, agent_id: &str, workspace_id: &str) {
    let schedule = client.tool("create_schedule",json!({"prompt":"scheduled task","cron":"0 0 1 1 *","provider":"codex/offline-model","name":"MCP schedule"}),Some(agent_id)).await;
    let schedule_id = schedule["schedule"]["id"].as_str().unwrap();
    client
        .tool("pause_schedule", json!({"id":schedule_id}), None)
        .await;
    client
        .tool(
            "update_schedule",
            json!({"id":schedule_id,"prompt":"updated schedule","maxRuns":2}),
            None,
        )
        .await;
    client
        .tool("resume_schedule", json!({"id":schedule_id}), None)
        .await;
    assert_eq!(
        client
            .tool("inspect_schedule", json!({"id":schedule_id}), None)
            .await["schedule"]["workspaceId"],
        workspace_id
    );
    client
        .tool("run_schedule_once", json!({"id":schedule_id}), None)
        .await;
    assert_eq!(
        client
            .tool("schedule_logs", json!({"id":schedule_id,"limit":1}), None)
            .await["runs"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let heartbeat = client
        .tool(
            "create_heartbeat",
            json!({"prompt":"heartbeat task","cron":"0 0 1 1 *"}),
            Some(agent_id),
        )
        .await;
    let heartbeat_id = heartbeat["schedule"]["id"].as_str().unwrap();
    assert_eq!(
        client.tool("list_schedules", json!({}), None).await["schedules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mismatch = client
        .rpc(
            "tools/call",
            json!({"name":"delete_heartbeat","arguments":{"id":heartbeat_id}}),
            None,
        )
        .await;
    assert_eq!(mismatch["result"]["isError"], true);
    client
        .tool(
            "delete_heartbeat",
            json!({"id":heartbeat_id}),
            Some(agent_id),
        )
        .await;
    client
        .tool("delete_schedule", json!({"id":schedule_id}), None)
        .await;
    client
        .tool(
            "archive_workspace",
            json!({"workspaceId":workspace_id}),
            None,
        )
        .await;
}
