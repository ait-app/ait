//! Workspace scope, reserved labels and the mode cap on every scoped Agent MCP entry.

use std::time::Duration;

use domain::ConversationId;
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_SCOPE_LABEL};

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
    pub(super) async fn awaiting_permission(&self, agent: &str) {
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
    let bot = scoped.owner_credential().await;
    let call = |tool: &'static str, arguments: Value| scoped.call(&bot, None, tool, arguments);

    let mut outside = create("Outside");
    outside["workspaceId"] = json!(scoped.other);
    forbidden(&call("agent_create", outside).await);

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
    forbidden(&call("agent_get", json!({"agentId":foreign})).await);
    // A prefix resolves to the foreign Agent; the resolved Workspace decides.
    forbidden(&call("agent_cancel", json!({"agentId":&foreign[..8]})).await);
    structured(&call("agent_get", json!({"agentId":&own[..8]})).await);

    let foreign_schedule = id(
        &scoped
            .call(
                TOKEN,
                None,
                "schedule_create",
                json!({"workspaceId":scoped.other,"provider":"codex","prompt":"report",
                    "cron":"0 9 * * *"}),
            )
            .await,
        "/schedule/id",
    );
    let own_schedule = id(
        &call(
            "schedule_create",
            json!({"provider":"codex","prompt":"report","cron":"0 9 * * *"}),
        )
        .await,
        "/schedule/id",
    );
    for tool in ["schedule_get", "schedule_pause", "schedule_log_read"] {
        forbidden(&call(tool, json!({"scheduleId":foreign_schedule})).await);
    }
    structured(&call("schedule_get", json!({"scheduleId":own_schedule})).await);

    let foreign_terminal = scoped.terminal(&scoped.other, &scoped.other_cwd).await;
    forbidden(&call("terminal_capture", json!({"terminalId":foreign_terminal})).await);
    forbidden(
        &call(
            "terminal_send",
            json!({"terminalId":foreign_terminal,"keys":"Enter"}),
        )
        .await,
    );
    forbidden(&call("terminal_kill", json!({"terminalId":foreign_terminal})).await);
    let own_terminal = id(&call("terminal_create", json!({})).await, "/terminal/id");
    structured(&call("terminal_capture", json!({"terminalId":own_terminal})).await);
    let terminals = call("terminal_list", json!({})).await;
    assert_eq!(
        ids(structured(&terminals), "terminals"),
        [own_terminal.as_str()]
    );
    let listed = call("terminal_list", json!({"workspaceId":scoped.other})).await;
    forbidden(&listed);

    let agents = call("agent_list", json!({})).await;
    assert_eq!(ids(structured(&agents), "agents"), [own.as_str()]);
    let pending = call("permission_list", json!({})).await;
    assert_eq!(ids(structured(&pending), "permissions"), [own.as_str()]);
    let schedules = call("schedule_list", json!({})).await;
    assert_eq!(
        ids(structured(&schedules), "schedules"),
        [own_schedule.as_str()]
    );

    let cwd = json!(scoped.cwd);
    forbidden(&call("terminal_create", json!({"cwd":cwd})).await);
    forbidden(&call("provider_model_list", json!({"provider":"codex","cwd":cwd})).await);
    structured(&call("provider_model_list", json!({"provider":"codex"})).await);
    scoped.stop().await;
}

#[tokio::test]
async fn subtree_child_inherits_scope_and_cannot_escalate() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let conversation = ConversationId::generate().to_string();
    let bot = scoped.bot_credential_in(conversation.parse().unwrap());
    let created = scoped
        .call(&bot, None, "agent_create", create("Child"))
        .await;
    let child = id(&created, "/agent/id");
    let labels = scoped.labels(&child).await;
    assert_eq!(labels[BOT_SCOPE_LABEL], scoped.workspace);
    assert_eq!(labels[BOT_CONVERSATION_LABEL], conversation, "{labels}");
    assert!(labels.get(PARENT_LABEL).is_none(), "{labels}");

    let credential = scoped.subtree_credential(&child);
    rejected(
        &scoped
            .call(
                &credential,
                None,
                "permission_respond",
                json!({"agentId":child,"requestId":"request","response":{}}),
            )
            .await,
        "Unknown or unavailable tool",
    );
    let mut escalated = create("Grandchild");
    escalated["settings"] = json!({"modeId":"full-access"});
    forbidden(
        &scoped
            .call(&credential, None, "agent_create", escalated)
            .await,
    );
    let grandchild = id(
        &scoped
            .call(&credential, None, "agent_create", create("Grandchild"))
            .await,
        "/agent/id",
    );
    let labels = scoped.labels(&grandchild).await;
    assert_eq!(labels[BOT_SCOPE_LABEL], scoped.workspace);
    assert_eq!(labels[BOT_CONVERSATION_LABEL], conversation, "{labels}");
    assert_eq!(labels[PARENT_LABEL], child);
    // Neither the subtree nor the host may name another origin conversation.
    let mut forged = create("Forged");
    forged["labels"] = json!({BOT_CONVERSATION_LABEL:ConversationId::generate()});
    forbidden(&scoped.call(&credential, None, "agent_create", forged).await);
    for token in [credential.as_str(), TOKEN] {
        forbidden(
            &scoped
                .call(
                    token,
                    None,
                    "agent_update",
                    json!({"agentId":grandchild,
                        "labels":{BOT_CONVERSATION_LABEL:ConversationId::generate()}}),
                )
                .await,
        );
    }
    let mut host = create("Host");
    host["workspaceId"] = json!(scoped.workspace);
    host["labels"] = json!({BOT_CONVERSATION_LABEL:ConversationId::generate()});
    forbidden(&scoped.call(TOKEN, None, "agent_create", host).await);
    forbidden(
        &scoped
            .call(
                &credential,
                None,
                "agent_update",
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
                "agent_update",
                json!({"agentId":grandchild,"labels":{"ait.bot.note":"x"}}),
            )
            .await,
    );
    let labels = scoped.labels(&grandchild).await;
    assert_eq!(labels[BOT_SCOPE_LABEL], scoped.workspace);
    assert_eq!(labels[BOT_CONVERSATION_LABEL], conversation, "{labels}");
    scoped.stop().await;
}

#[tokio::test]
async fn command_tools_reach_every_agent_in_scope_only() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto", "read-only"])).await;
    let bot = scoped.owner_credential().await;
    let call = |tool: &'static str, arguments: Value| scoped.call(&bot, None, tool, arguments);
    let user = scoped.agent(json!({})).await;
    let prefix = &user[..8];
    structured(&call("agent_prompt", json!({"agentId":prefix,"prompt":"hello"})).await);
    structured(&call("agent_mode_set", json!({"agentId":prefix,"modeId":"auto"})).await);
    forbidden(
        &call(
            "agent_mode_set",
            json!({"agentId":user,"modeId":"full-access"}),
        )
        .await,
    );
    forbidden(
        &call(
            "agent_update",
            json!({"agentId":user,"settings":{"modeId":"full-access"}}),
        )
        .await,
    );
    structured(&call("agent_update", json!({"agentId":prefix,"title":"Renamed"})).await);
    let status = call("agent_get", json!({"agentId":user})).await;
    assert_eq!(structured(&status)["snapshot"]["title"], "Renamed");
    let cancelled = call("agent_cancel", json!({"agentId":prefix})).await;
    assert_eq!(cancelled["result"]["isError"], false, "{cancelled}");

    let foreign = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    for (tool, arguments) in [
        ("agent_prompt", json!({"agentId":foreign,"prompt":"hello"})),
        ("agent_update", json!({"agentId":foreign,"title":"Renamed"})),
        (
            "agent_mode_set",
            json!({"agentId":foreign,"modeId":"read-only"}),
        ),
        ("agent_cancel", json!({"agentId":foreign})),
    ] {
        forbidden(&call(tool, arguments).await);
    }
    scoped.stop().await;
}

#[tokio::test]
async fn bots_and_their_agents_cannot_create_workspaces() {
    let scoped = Scoped::start().await;
    scoped
        .setup(permissions(
            &["observe", "agents", "stop", "workspaces"],
            &["auto"],
        ))
        .await;
    let bot = scoped.bot_credential();
    let before = scoped.workspace_count().await;
    for isolation in ["local", "worktree", "sandboxed-worktree"] {
        let refused = scoped
            .call(
                &bot,
                None,
                "workspace_create",
                json!({"isolation":isolation}),
            )
            .await;
        assert_eq!(
            refused["error"]["message"], "Unknown or unavailable tool",
            "{refused}"
        );
    }
    assert_eq!(scoped.workspace_count().await, before);
    let created = scoped
        .call(
            TOKEN,
            None,
            "workspace_create",
            json!({"isolation":"worktree","path":scoped.cwd}),
        )
        .await;
    assert_eq!(scoped.workspace_count().await, before + 1, "{created}");
    scoped.stop().await;
}
