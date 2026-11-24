//! Tool calls a sibling Host sends over a peer session: the receiving Host's rules for them.

use model::peer::{Identity, Request};
use relay::hub::bridge::{LocalApi, PeerRefusal};
use uuid::Uuid;

use super::support::forbidden;
use super::*;
use crate::connection::bridge::InProcess;
use crate::mcp::peer::{PARENT_AGENT_LABEL, PARENT_SERVER_LABEL, serve};

/// The local parent label a remote parent must never be recorded under.
const LOCAL_PARENT_LABEL: &str = "paseo.parent-agent-id";

fn caller_host() -> Identity {
    Identity {
        host_id: Uuid::new_v4(),
        server_id: Uuid::new_v4(),
        name: "Workstation".to_owned(),
    }
}

fn request(tool: &str, arguments: Value, caller: Option<&str>) -> Request {
    Request {
        tool: tool.to_owned(),
        arguments,
        caller: caller.map(str::to_owned),
    }
}

/// An `agent_create` a peer may send: placed in Workspace `workspace`.
fn create_in(workspace: &str) -> Value {
    json!({"workspaceId":workspace,"provider":"codex/offline-model","title":"Remote child",
        "initialPrompt":"work"})
}

fn refused(result: &Result<Value, PeerRefusal>, message: &str) {
    assert_eq!(
        *result,
        Err(PeerRefusal::Failed {
            code: -32602,
            message: message.to_owned()
        })
    );
}

/// The text of a tool result that failed, as a peer receives it.
fn failure(result: Result<Value, PeerRefusal>) -> String {
    let result = result.expect("a tool result");
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{result}"))
        .to_owned()
}

impl Scoped {
    async fn configure(&self, mcp: Value) {
        self.rpc("daemon.config.set.request", json!({"config":{"mcp":mcp}}))
            .await;
    }

    async fn accept_peers(&self) {
        self.configure(json!({"acceptPeers":true})).await;
    }

    async fn peer(&self, from: &Identity, request: Request) -> Result<Value, PeerRefusal> {
        serve(&self.fixture.api.shared, from, request).await
    }

    async fn agent_count(&self) -> usize {
        let listed = self.call(TOKEN, None, "agent_list", json!({})).await;
        structured(&listed)["agents"]
            .as_array()
            .unwrap_or_else(|| panic!("{listed}"))
            .len()
    }

    async fn agent_labels(&self, agent: &str) -> Value {
        self.rpc("agent.get.request", json!({"agentId":agent}))
            .await["agent"]["labels"]
            .clone()
    }
}

#[tokio::test]
async fn a_peer_creates_an_agent_with_its_remote_parent_labels() {
    let scoped = Scoped::start().await;
    scoped.accept_peers().await;
    let from = caller_host();
    let mut arguments = create_in(&scoped.workspace);
    arguments["labels"] = json!({"team":"remote"});

    let created = scoped
        .peer(&from, request("agent_create", arguments, Some("a-1")))
        .await
        .expect("a tool result");

    assert_eq!(created["isError"], false, "{created}");
    assert_eq!(
        created["content"][0]["text"],
        created["structuredContent"].to_string(),
        "the result has the shape of a local tools/call result"
    );
    let agent = created["structuredContent"]["agentId"]
        .as_str()
        .unwrap_or_else(|| panic!("{created}"));
    let labels = scoped.agent_labels(agent).await;
    assert_eq!(labels[PARENT_SERVER_LABEL], from.server_id.to_string());
    assert_eq!(labels[PARENT_AGENT_LABEL], "a-1");
    assert_eq!(labels["team"], "remote", "the peer's own labels are kept");
    assert!(labels.get(LOCAL_PARENT_LABEL).is_none(), "{labels}");

    // Through the Hub bridge, and without a calling Agent: no parent is recorded.
    let orphan = InProcess::new(&scoped.fixture.api.shared)
        .peer(
            &from,
            request("agent_create", create_in(&scoped.workspace), None),
        )
        .await
        .expect("a tool result");
    let orphan = orphan["structuredContent"]["agent"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{orphan}"));
    let labels = scoped.agent_labels(orphan).await;
    for label in [PARENT_SERVER_LABEL, PARENT_AGENT_LABEL, LOCAL_PARENT_LABEL] {
        assert!(labels.get(label).is_none(), "{label}: {labels}");
    }
    scoped.stop().await;
}

#[tokio::test]
async fn peers_are_confined_to_their_rules() {
    let scoped = Scoped::start().await;
    scoped.accept_peers().await;
    let from = caller_host();
    let created = scoped.agent_count().await;

    // Only current names of tools marked for peers.
    for tool in [
        "terminal_create",
        "host_list",
        "permission_respond",
        "create_agent",
    ] {
        refused(
            &scoped.peer(&from, request(tool, json!({}), None)).await,
            "Unknown or unavailable tool",
        );
    }
    // No second hop, and the host credential's schema without Bot-only arguments.
    let mut second_hop = create_in(&scoped.workspace);
    second_hop["hostId"] = json!(Uuid::new_v4());
    let mut ticket = create_in(&scoped.workspace);
    ticket["ticketId"] = json!("ticket");
    let mut parentage = create_in(&scoped.workspace);
    parentage["callerAgentId"] = json!("a-1");
    for arguments in [second_hop, ticket, parentage] {
        refused(
            &scoped
                .peer(&from, request("agent_create", arguments, Some("a-1")))
                .await,
            "Arguments do not match the tool input schema",
        );
    }
    refused(
        &scoped
            .peer(&from, request("agent_list", json!({"hostId":"x"}), None))
            .await,
        "Arguments do not match the tool input schema",
    );

    // Nothing to inherit placement from.
    let mut unplaced = create_in(&scoped.workspace);
    unplaced.as_object_mut().unwrap().remove("workspaceId");
    let text = failure(
        scoped
            .peer(&from, request("agent_create", unplaced, Some("a-1")))
            .await,
    );
    assert!(text.contains("workspaceId"), "{text}");
    let mut directory = create_in(&scoped.workspace);
    directory["cwd"] = json!(scoped.cwd);
    let text = failure(
        scoped
            .peer(&from, request("agent_create", directory, Some("a-1")))
            .await,
    );
    assert!(text.contains("cwd"), "{text}");
    let text = failure(
        scoped
            .peer(
                &from,
                request("workspace_create", json!({"isolation":"worktree"}), None),
            )
            .await,
    );
    assert!(
        text.contains("path") && text.contains("projectId"),
        "{text}"
    );

    // Labels in Ait's and Paseo's namespaces are written by this Host only.
    for labels in [
        json!({"ait.x":"1"}),
        json!({"paseo.parent-agent-id":"x"}),
        json!({PARENT_AGENT_LABEL:"x"}),
    ] {
        let mut arguments = create_in(&scoped.workspace);
        arguments["labels"] = labels;
        let text = failure(
            scoped
                .peer(&from, request("agent_create", arguments, Some("a-1")))
                .await,
        );
        assert_eq!(text, model::ErrorCode::Forbidden.message());
    }
    assert_eq!(scoped.agent_count().await, created, "nothing was created");

    // An explicit source is enough to create a Workspace.
    let workspace = scoped
        .peer(
            &from,
            request(
                "workspace_create",
                json!({"isolation":"local","path":scoped.other_cwd}),
                None,
            ),
        )
        .await
        .expect("a tool result");
    assert_eq!(workspace["isError"], false, "{workspace}");
    scoped.stop().await;
}

#[tokio::test]
async fn a_host_that_stopped_accepting_refuses_established_peers() {
    let scoped = Scoped::start().await;
    let from = caller_host();
    let create = || request("agent_create", create_in(&scoped.workspace), Some("a-1"));
    let created = scoped.agent_count().await;

    assert_eq!(
        scoped.peer(&from, create()).await,
        Err(PeerRefusal::Disabled),
        "a new Host does not accept"
    );
    scoped.accept_peers().await;
    scoped.configure(json!({"enabled":false})).await;
    assert_eq!(
        scoped.peer(&from, create()).await,
        Err(PeerRefusal::Disabled),
        "a disabled MCP endpoint accepts no peers"
    );
    scoped.configure(json!({"enabled":true})).await;
    scoped.configure(json!({"acceptPeers":false})).await;
    assert_eq!(
        scoped
            .peer(&from, request("agent_list", json!({}), None))
            .await,
        Err(PeerRefusal::Disabled),
        "acceptance ends with the setting, whatever the center last heard"
    );
    assert_eq!(scoped.agent_count().await, created, "nothing was created");
    scoped.stop().await;
}

#[tokio::test]
async fn a_peer_is_refused_rather_than_queued_when_the_call_budget_is_full() {
    let scoped = Scoped::start().await;
    scoped.accept_peers().await;
    let connections = scoped.fixture.api.shared.connections.clone();
    let held = connections
        .clone()
        .try_acquire_many_owned(u32::try_from(connections.available_permits()).unwrap())
        .expect("every call permit");

    assert_eq!(
        scoped
            .peer(&caller_host(), request("agent_list", json!({}), None))
            .await,
        Err(PeerRefusal::Failed {
            code: -32000,
            message: "MCP request budget exhausted".to_owned()
        })
    );
    drop(held);
    scoped.stop().await;
}

#[tokio::test]
async fn nobody_else_writes_the_parent_labels() {
    let scoped = Scoped::start().await;
    let agent = scoped.agent(json!({})).await;
    for label in [PARENT_SERVER_LABEL, PARENT_AGENT_LABEL] {
        let mut create = create_in(&scoped.workspace);
        create["labels"] = json!({label:"x"});
        forbidden(&scoped.call(TOKEN, None, "agent_create", create).await);
        forbidden(
            &scoped
                .call(
                    TOKEN,
                    None,
                    "agent_update",
                    json!({"agentId":agent,"labels":{label:"x"}}),
                )
                .await,
        );
    }
    assert!(
        scoped
            .agent_labels(&agent)
            .await
            .get(PARENT_AGENT_LABEL)
            .is_none(),
        "the refused update changed nothing"
    );
    scoped.stop().await;
}
