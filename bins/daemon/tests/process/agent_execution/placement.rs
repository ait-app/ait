//! Native server creation placement, including a fresh Workspace for an unscoped human request.

use super::*;

#[tokio::test]
async fn websocket_agent_creation_resolves_fresh_explicit_and_caller_workspaces() {
    let super::super::native::NativeFixture { root, cwd, path } =
        super::super::native::NativeFixture::new();
    let state = root.path().join("state");
    let log = root.path().join("server.log");
    let mut process = start_with_path(&state, &log, Some(&path));
    let address = ready(&mut process, &log).await;
    let mut methods = METHODS.to_vec();
    methods.push("workspace.list.request");
    let mut client = connect(&address, &methods).await;
    let params = json!({"idempotencyKey":"first-human", "config":{"provider":"codex","cwd":cwd,"title":"First"}});
    let first = request(&mut client, "agent.create.request", params.clone()).await;
    assert_eq!(first["type"], "response", "{first}");
    let repeated = request(&mut client, "agent.create.request", params).await;
    assert_eq!(first["result"]["agentId"], repeated["result"]["agentId"]);
    let second = request(
        &mut client,
        "agent.create.request",
        json!({"config":{"provider":"codex","cwd":cwd,"title":"Second"}}),
    )
    .await;
    assert_eq!(second["type"], "response", "{second}");
    let first_workspace = &first["result"]["agent"]["workspaceId"];
    let second_workspace = &second["result"]["agent"]["workspaceId"];
    assert_ne!(first_workspace, second_workspace);
    let parent = &second["result"]["agentId"];
    for explicit in [true, false] {
        let mut params = json!({"callerAgentId":parent,"config":{"provider":"codex","cwd":"/missing/stale-draft"}});
        if explicit {
            params["workspaceId"] = first_workspace.clone();
        }
        let child = request(&mut client, "agent.create.request", params).await;
        assert_eq!(child["type"], "response", "{child}");
        assert_eq!(
            &child["result"]["agent"]["workspaceId"],
            if explicit {
                first_workspace
            } else {
                second_workspace
            }
        );
        assert_eq!(
            &child["result"]["agent"]["labels"]["paseo.parent-agent-id"],
            parent
        );
    }
    let workspaces = request(&mut client, "workspace.list.request", json!({})).await;
    assert_eq!(workspaces["result"]["entries"].as_array().unwrap().len(), 2);
    client.close(None).await.unwrap();
    terminate(&mut process).await;
    assert_children_exited(&cwd).await;
}
