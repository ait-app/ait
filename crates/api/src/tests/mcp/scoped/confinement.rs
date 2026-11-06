//! Workspace scope, reserved labels and the mode cap on every scoped Agent MCP entry.

use std::time::Duration;

use domain::bot::BOT_SCOPE_LABEL;

use super::support::{forbidden, structured};
use super::*;

const PARENT_LABEL: &str = "paseo.parent-agent-id";

fn permissions(groups: &[&str], modes: &[&str]) -> Value {
    json!({"groups":groups,"modes":{"codex":modes}})
}

fn create(title: &str) -> Value {
    json!({"provider":"codex/offline-model","title":title,"initialPrompt":"work"})
}

fn id(response: &Value, pointer: &str) -> String {
    structured(response)
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{pointer}: {response}"))
        .to_owned()
}

fn ids<'a>(entries: &'a Value, key: &str) -> Vec<&'a str> {
    entries[key]
        .as_array()
        .unwrap_or_else(|| panic!("{key}: {entries}"))
        .iter()
        .filter_map(|entry| entry["id"].as_str().or_else(|| entry["agentId"].as_str()))
        .collect()
}

impl Scoped {
    async fn labels(&self, agent: &str) -> Value {
        self.rpc("agent.get.request", json!({"agentId":agent}))
            .await["agent"]["labels"]
            .clone()
    }

    /// Wait until `agent` reports a pending permission request.
    async fn awaiting_permission(&self, agent: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let value = self
                    .rpc("agent.get.request", json!({"agentId":agent}))
                    .await;
                if value["agent"]["pendingPermissions"]
                    .as_array()
                    .is_some_and(|pending| !pending.is_empty())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{agent} never asked for permission"));
    }

    async fn workspace_count(&self) -> usize {
        self.rpc("workspace.list.request", json!({})).await["entries"]
            .as_array()
            .unwrap()
            .len()
    }
}

#[tokio::test]
async fn scoped_calls_cannot_reach_other_workspaces() {
    let scoped = Scoped::start().await;
    scoped
        .setup(permissions(
            &["observe", "agents", "stop", "schedules", "terminals"],
            &["auto"],
        ))
        .await;
    let bot = scoped.bot_credential();
    let call = |tool: &'static str, arguments: Value| scoped.call(&bot, None, tool, arguments);

    let mut outside = create("Outside");
    outside["workspaceId"] = json!(scoped.other);
    forbidden(&call("create_agent", outside).await);

    let own = scoped
        .agent_in(
            &scoped.workspace,
            &scoped.cwd,
            json!({}),
            Some("permit-command"),
        )
        .await;
    let foreign = scoped
        .agent_in(
            &scoped.other,
            &scoped.other_cwd,
            json!({}),
            Some("permit-command"),
        )
        .await;
    for agent in [&own, &foreign] {
        scoped.awaiting_permission(agent).await;
    }
    forbidden(&call("get_agent_status", json!({"agentId":foreign})).await);
    // A prefix resolves to the foreign Agent; the resolved Workspace decides.
    forbidden(&call("cancel_agent", json!({"agentId":&foreign[..8]})).await);
    structured(&call("get_agent_status", json!({"agentId":&own[..8]})).await);

    let foreign_schedule = id(
        &scoped
            .call(
                TOKEN,
                None,
                "create_schedule",
                json!({"workspaceId":scoped.other,"provider":"codex","prompt":"report",
                    "cron":"0 9 * * *"}),
            )
            .await,
        "/schedule/id",
    );
    let own_schedule = id(
        &call(
            "create_schedule",
            json!({"provider":"codex","prompt":"report","cron":"0 9 * * *"}),
        )
        .await,
        "/schedule/id",
    );
    for tool in ["inspect_schedule", "pause_schedule", "schedule_logs"] {
        forbidden(&call(tool, json!({"id":foreign_schedule})).await);
    }
    structured(&call("inspect_schedule", json!({"id":own_schedule})).await);

    let foreign_terminal = scoped.terminal(&scoped.other, &scoped.other_cwd).await;
    forbidden(&call("capture_terminal", json!({"terminalId":foreign_terminal})).await);
    forbidden(
        &call(
            "send_terminal_keys",
            json!({"terminalId":foreign_terminal,"keys":"Enter"}),
        )
        .await,
    );
    forbidden(&call("kill_terminal", json!({"terminalId":foreign_terminal})).await);
    let own_terminal = id(&call("create_terminal", json!({})).await, "/terminal/id");
    structured(&call("capture_terminal", json!({"terminalId":own_terminal})).await);
    let terminals = call("list_terminals", json!({})).await;
    assert_eq!(
        ids(structured(&terminals), "terminals"),
        [own_terminal.as_str()]
    );
    let listed = call("list_terminals", json!({"workspaceId":scoped.other})).await;
    forbidden(&listed);

    let agents = call("list_agents", json!({})).await;
    assert_eq!(ids(structured(&agents), "agents"), [own.as_str()]);
    let pending = call("list_pending_permissions", json!({})).await;
    assert_eq!(ids(structured(&pending), "permissions"), [own.as_str()]);
    let schedules = call("list_schedules", json!({})).await;
    assert_eq!(
        ids(structured(&schedules), "schedules"),
        [own_schedule.as_str()]
    );

    let cwd = json!(scoped.cwd);
    forbidden(&call("create_terminal", json!({"cwd":cwd})).await);
    forbidden(&call("list_models", json!({"provider":"codex","cwd":cwd})).await);
    structured(&call("list_models", json!({"provider":"codex"})).await);
    scoped.stop().await;
}

#[tokio::test]
async fn subtree_child_inherits_scope_and_cannot_escalate() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let bot = scoped.bot_credential();
    let created = scoped
        .call(&bot, None, "create_agent", create("Child"))
        .await;
    let child = id(&created, "/agent/id");
    let labels = scoped.labels(&child).await;
    assert_eq!(labels[BOT_SCOPE_LABEL], scoped.workspace);
    assert!(labels.get(PARENT_LABEL).is_none(), "{labels}");

    let credential = scoped.subtree_credential(&child);
    rejected(
        &scoped
            .call(
                &credential,
                None,
                "respond_to_permission",
                json!({"agentId":child,"requestId":"request","response":{}}),
            )
            .await,
        "Unknown or unavailable tool",
    );
    let mut escalated = create("Grandchild");
    escalated["settings"] = json!({"modeId":"full-access"});
    forbidden(
        &scoped
            .call(&credential, None, "create_agent", escalated)
            .await,
    );
    let grandchild = id(
        &scoped
            .call(&credential, None, "create_agent", create("Grandchild"))
            .await,
        "/agent/id",
    );
    let labels = scoped.labels(&grandchild).await;
    assert_eq!(labels[BOT_SCOPE_LABEL], scoped.workspace);
    assert_eq!(labels[PARENT_LABEL], child);
    forbidden(
        &scoped
            .call(
                &credential,
                None,
                "update_agent",
                json!({"agentId":grandchild,"labels":{BOT_SCOPE_LABEL:scoped.other}}),
            )
            .await,
    );
    // The host may not forge the reserved namespace through MCP either.
    forbidden(
        &scoped
            .call(
                TOKEN,
                None,
                "update_agent",
                json!({"agentId":grandchild,"labels":{"ait.bot.note":"x"}}),
            )
            .await,
    );
    assert_eq!(
        scoped.labels(&grandchild).await[BOT_SCOPE_LABEL],
        scoped.workspace
    );
    scoped.stop().await;
}

#[tokio::test]
async fn command_tools_reach_every_agent_in_scope_only() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto", "read-only"])).await;
    let bot = scoped.bot_credential();
    let call = |tool: &'static str, arguments: Value| scoped.call(&bot, None, tool, arguments);
    let user = scoped.agent(json!({})).await;
    let prefix = &user[..8];
    structured(
        &call(
            "send_agent_prompt",
            json!({"agentId":prefix,"prompt":"hello"}),
        )
        .await,
    );
    structured(
        &call(
            "set_agent_mode",
            json!({"agentId":prefix,"modeId":"read-only"}),
        )
        .await,
    );
    forbidden(
        &call(
            "set_agent_mode",
            json!({"agentId":user,"modeId":"full-access"}),
        )
        .await,
    );
    forbidden(
        &call(
            "update_agent",
            json!({"agentId":user,"settings":{"modeId":"full-access"}}),
        )
        .await,
    );
    structured(&call("update_agent", json!({"agentId":prefix,"name":"Renamed"})).await);
    let status = call("get_agent_status", json!({"agentId":user})).await;
    assert_eq!(structured(&status)["snapshot"]["title"], "Renamed");
    let cancelled = call("cancel_agent", json!({"agentId":prefix})).await;
    assert_eq!(cancelled["result"]["isError"], false, "{cancelled}");

    let foreign = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    for (tool, arguments) in [
        (
            "send_agent_prompt",
            json!({"agentId":foreign,"prompt":"hello"}),
        ),
        ("update_agent", json!({"agentId":foreign,"name":"Renamed"})),
        (
            "set_agent_mode",
            json!({"agentId":foreign,"modeId":"read-only"}),
        ),
        ("cancel_agent", json!({"agentId":foreign})),
    ] {
        forbidden(&call(tool, arguments).await);
    }
    scoped.stop().await;
}

#[tokio::test]
async fn bot_created_worktree_extends_scope() {
    let scoped = Scoped::start().await;
    scoped
        .setup(permissions(
            &["observe", "agents", "stop", "workspaces"],
            &["auto"],
        ))
        .await;
    let bot = scoped.bot_credential();
    let call = |tool: &'static str, arguments: Value| scoped.call(&bot, None, tool, arguments);
    forbidden(&call("create_workspace", json!({"isolation":"local"})).await);
    forbidden(
        &call(
            "create_workspace",
            json!({"isolation":"worktree","idempotencyKey":"replayed"}),
        )
        .await,
    );
    let created = call(
        "create_workspace",
        json!({"isolation":"worktree","path":scoped.other_cwd}),
    )
    .await;
    let worktree = id(&created, "/workspace/id");
    let unpruned = id(
        &call("create_workspace", json!({"isolation":"worktree"})).await,
        "/workspace/id",
    );
    assert_eq!(
        structured(&created)["workspace"]["projectId"],
        scoped
            .rpc(
                "workspace.get.request",
                json!({"workspaceId":scoped.workspace})
            )
            .await["workspace"]["projectId"]
    );
    let mut placed = create("In worktree");
    placed["workspaceId"] = json!(worktree);
    structured(&call("create_agent", placed.clone()).await);

    let unrelated = id(
        &scoped
            .call(
                TOKEN,
                None,
                "create_workspace",
                json!({"isolation":"worktree","path":scoped.cwd}),
            )
            .await,
        "/workspace/id",
    );
    let mut elsewhere = create("Elsewhere");
    elsewhere["workspaceId"] = json!(unrelated);
    forbidden(&call("create_agent", elsewhere).await);

    scoped.archive(&worktree).await;
    forbidden(&call("create_agent", placed).await);
    let mut stale = create("Stale grant");
    stale["workspaceId"] = json!(unpruned);
    structured(&call("create_agent", stale.clone()).await);
    scoped.archive_unpruned(&unpruned);
    assert!(
        scoped
            .scopes
            .grant(&scoped.workspace)
            .workspaces
            .contains(&unpruned)
    );
    forbidden(&call("create_agent", stale).await);

    for index in 0..domain::bot::BOT_WORKSPACES {
        let recorded = scoped
            .bots()
            .record_workspace(&scoped.workspace, &format!("wks_recorded_{index}"))
            .await;
        if recorded == Err(model::ErrorCode::ResourceExhausted) {
            break;
        }
        recorded.unwrap();
    }
    let before = scoped.workspace_count().await;
    let exhausted = call("create_workspace", json!({"isolation":"worktree"})).await;
    assert_eq!(exhausted["result"]["isError"], true, "{exhausted}");
    assert_eq!(
        exhausted["result"]["content"][0]["text"],
        model::ErrorCode::ResourceExhausted.message()
    );
    assert_eq!(scoped.workspace_count().await, before);
    scoped.stop().await;
}
