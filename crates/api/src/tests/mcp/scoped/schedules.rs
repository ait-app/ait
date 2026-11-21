//! Schedules and heartbeats created or controlled by scoped callers stay within the Bot's grant.

use domain::ConversationId;
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_SCOPE_LABEL};

use super::support::{forbidden, structured};
use super::*;

const CRON: &str = "0 9 * * *";

impl Scoped {
    /// Create a schedule through the host RPC in `workspace` with new-Agent `config`.
    async fn user_schedule(&self, workspace: &str, config: Value) -> String {
        let created = self
            .rpc(
                "schedule.create.request",
                json!({"workspaceId":workspace,"prompt":"report",
                    "cadence":{"type":"cron","expression":CRON},
                    "target":{"type":"new-agent","config":config}}),
            )
            .await;
        created["schedule"]["id"].as_str().unwrap().to_owned()
    }

    /// The stored schedule document `id`.
    async fn stored(&self, id: &str) -> Value {
        self.rpc("schedule.inspect.request", json!({"scheduleId":id}))
            .await["schedule"]
            .clone()
    }
}

/// Assert that a tool call was refused because its schedule stores no explicit mode.
fn unset_mode(response: &Value) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("no explicit mode"), "{response}");
    assert!(text.contains("ask the user"), "{response}");
}

/// Assert that a tool call was refused because the schedule was not created by the Bot.
fn user_owned(response: &Value) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        text.contains("Only schedules created by the Workspace Bot can be edited"),
        "{response}"
    );
}

fn schedule_id(response: &Value) -> String {
    structured(response)["schedule"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{response}"))
        .to_owned()
}

#[tokio::test]
async fn scoped_schedule_create_sets_bot_scope_and_default_mode() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto", "read-only"])).await;
    let conversation = ConversationId::generate();
    let bot = scoped.bot_credential_in(conversation);

    let created = scoped
        .call(
            &bot,
            None,
            "schedule_create",
            json!({"provider":"codex/offline-model","prompt":"report","cron":CRON}),
        )
        .await;
    let stored = scoped.stored(&schedule_id(&created)).await;
    assert_eq!(stored["botScope"], scoped.workspace.as_str(), "{stored}");
    assert_eq!(stored["botConversation"], json!(conversation), "{stored}");
    assert_eq!(stored["workspaceId"], scoped.workspace.as_str());
    assert_eq!(stored["target"]["config"]["modeId"], "auto", "{stored}");
    assert_eq!(stored["target"]["config"]["model"], "offline-model");

    for provider in ["claude", "claude/model"] {
        forbidden(
            &scoped
                .call(
                    &bot,
                    None,
                    "schedule_create",
                    json!({"provider":provider,"prompt":"report","cron":CRON}),
                )
                .await,
        );
    }
    forbidden(
        &scoped
            .call(
                &bot,
                None,
                "schedule_create",
                json!({"workspaceId":scoped.other,"provider":"codex","prompt":"report",
                    "cron":CRON}),
            )
            .await,
    );

    // A subtree caller inherits its own provider when none is named.
    let child = scoped.subtree_agent().await;
    let credential = scoped.subtree_credential(&child);
    let created = scoped
        .call(
            &credential,
            None,
            "schedule_create",
            json!({"prompt":"report","cron":CRON}),
        )
        .await;
    let stored = scoped.stored(&schedule_id(&created)).await;
    assert_eq!(stored["botScope"], scoped.workspace.as_str(), "{stored}");
    assert!(stored.get("botConversation").is_none(), "{stored}");
    assert_eq!(stored["target"]["config"]["provider"], "codex");
    assert_eq!(stored["target"]["config"]["modeId"], "auto");

    // A subtree caller started from a conversation passes that origin on.
    let labelled = scoped
        .agent(json!({BOT_SCOPE_LABEL:scoped.workspace,BOT_CONVERSATION_LABEL:conversation}))
        .await;
    let created = scoped
        .call(
            &scoped.subtree_credential(&labelled),
            None,
            "schedule_create",
            json!({"prompt":"report","cron":CRON}),
        )
        .await;
    let stored = scoped.stored(&schedule_id(&created)).await;
    assert_eq!(stored["botConversation"], json!(conversation), "{stored}");

    // The tool schema accepts no origin from any caller.
    let forged = scoped
        .call(
            TOKEN,
            None,
            "schedule_create",
            json!({"workspaceId":scoped.workspace,"provider":"codex","prompt":"report",
                "cron":CRON,"botConversation":ConversationId::generate()}),
        )
        .await;
    rejected(&forged, "Arguments do not match the tool input schema");

    // Host-created schedules carry no Bot scope.
    let host = scoped
        .call(
            TOKEN,
            None,
            "schedule_create",
            json!({"workspaceId":scoped.workspace,"provider":"codex","prompt":"report",
                "cron":CRON}),
        )
        .await;
    let stored = scoped.stored(&schedule_id(&host)).await;
    assert!(stored.get("botScope").is_none(), "{stored}");
    assert!(
        stored["target"]["config"].get("modeId").is_none(),
        "{stored}"
    );
    scoped.stop().await;
}

#[tokio::test]
async fn scoped_update_schedule_enforces_mode_cap() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let bot = scoped.bot_credential();
    let created = scoped
        .call(
            &bot,
            None,
            "schedule_create",
            json!({"provider":"codex","prompt":"report","cron":CRON}),
        )
        .await;
    let own = schedule_id(&created);
    let update = |id: &str, patch: Value| {
        let mut arguments = patch;
        arguments["scheduleId"] = json!(id);
        scoped.call(&bot, None, "schedule_update", arguments)
    };

    forbidden(&update(&own, json!({"modeId":"full-access"})).await);
    forbidden(&update(&own, json!({"provider":"claude"})).await);
    forbidden(&update(&own, json!({"provider":"claude/model","modeId":"auto"})).await);
    structured(&update(&own, json!({"modeId":null})).await);
    let stored = scoped.stored(&own).await;
    assert_eq!(stored["target"]["config"]["modeId"], "auto", "{stored}");
    assert_eq!(stored["botScope"], scoped.workspace.as_str());

    structured(&update(&own, json!({"prompt":"changed"})).await);
    assert_eq!(scoped.stored(&own).await["prompt"], "changed");

    // The user's own tasks are never edited, even within the cap.
    let user = scoped
        .user_schedule(
            &scoped.workspace,
            json!({"provider":"codex","modeId":"auto"}),
        )
        .await;
    for patch in [
        json!({"prompt":"changed"}),
        json!({"modeId":"auto"}),
        json!({"provider":"codex/other-model"}),
    ] {
        user_owned(&update(&user, patch).await);
    }
    let stored = scoped.stored(&user).await;
    assert_eq!(stored["prompt"], "report");
    assert_eq!(
        stored["target"]["config"],
        json!({"provider":"codex","modeId":"auto"}),
        "{stored}"
    );

    let foreign = scoped
        .user_schedule(&scoped.other, json!({"provider":"codex","modeId":"auto"}))
        .await;
    forbidden(&update(&foreign, json!({"prompt":"changed"})).await);
    assert_eq!(scoped.stored(&foreign).await["prompt"], "report");
    scoped.stop().await;
}

#[tokio::test]
async fn scoped_schedule_controls_check_target_mode() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let bot = scoped.bot_credential();
    let call = |tool: &'static str, id: &str, patch: Value| {
        let mut arguments = patch;
        arguments["scheduleId"] = json!(id);
        scoped.call(&bot, None, tool, arguments)
    };

    let escalated = scoped
        .user_schedule(
            &scoped.workspace,
            json!({"provider":"codex","modeId":"full-access"}),
        )
        .await;
    let unlisted = scoped
        .user_schedule(&scoped.workspace, json!({"provider":"claude"}))
        .await;
    for task in [&escalated, &unlisted] {
        scoped
            .rpc("schedule.pause.request", json!({"scheduleId":task}))
            .await;
        forbidden(&call("schedule_run", task, json!({})).await);
        forbidden(&call("schedule_resume", task, json!({})).await);
        user_owned(&call("schedule_update", task, json!({"prompt":"changed"})).await);
        let stored = scoped.stored(task).await;
        assert_eq!(stored["status"], "paused", "{stored}");
        assert_eq!(stored["prompt"], "report");
        assert_eq!(stored["runs"], json!([]));
        structured(&call("schedule_pause", task, json!({})).await);
        structured(&call("schedule_get", task, json!({})).await);
    }

    let allowed = scoped
        .user_schedule(
            &scoped.workspace,
            json!({"provider":"codex","modeId":"auto"}),
        )
        .await;
    user_owned(&call("schedule_update", &allowed, json!({"prompt":"changed"})).await);
    structured(&call("schedule_pause", &allowed, json!({})).await);
    structured(&call("schedule_resume", &allowed, json!({})).await);
    structured(&call("schedule_run", &allowed, json!({})).await);
    let stored = scoped.stored(&allowed).await;
    assert_eq!(stored["prompt"], "report");
    assert_eq!(stored["runs"].as_array().unwrap().len(), 1, "{stored}");

    // A user task without a stored mode would run at the provider's native default.
    let unset = scoped
        .user_schedule(&scoped.workspace, json!({"provider":"codex"}))
        .await;
    unset_mode(&call("schedule_run", &unset, json!({})).await);
    unset_mode(&call("schedule_resume", &unset, json!({})).await);
    user_owned(&call("schedule_update", &unset, json!({"modeId":"auto"})).await);
    let stored = scoped.stored(&unset).await;
    assert_eq!(stored["prompt"], "report");
    assert_eq!(stored["target"]["config"], json!({"provider":"codex"}));
    assert_eq!(stored["runs"], json!([]));

    // Once the user sets an allowed mode, the Bot may run a mode-less or escalated task.
    for task in [&unset, &escalated] {
        scoped
            .rpc(
                "schedule.update.request",
                json!({"scheduleId":task,"newAgentConfig":{"modeId":"auto"}}),
            )
            .await;
        structured(&call("schedule_run", task, json!({})).await);
        let stored = scoped.stored(task).await;
        assert_eq!(stored["target"]["config"]["modeId"], "auto", "{stored}");
        assert_eq!(stored["runs"].as_array().unwrap().len(), 1, "{stored}");
    }

    // A Bot-created task without a stored mode runs at the cap's default mode.
    let own = schedule_id(
        &scoped
            .call(
                &bot,
                None,
                "schedule_create",
                json!({"provider":"codex","prompt":"report","cron":CRON}),
            )
            .await,
    );
    scoped
        .rpc(
            "schedule.update.request",
            json!({"scheduleId":own,"newAgentConfig":{"modeId":null}}),
        )
        .await;
    assert!(
        scoped.stored(&own).await["target"]["config"]
            .get("modeId")
            .is_none()
    );
    structured(&call("schedule_run", &own, json!({})).await);
    scoped.stop().await;
}

#[tokio::test]
async fn bot_cannot_create_heartbeat_targeting_itself() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let heartbeat = json!({"prompt":"check in","cron":CRON});
    forbidden(
        &scoped
            .call(
                &scoped.bot_credential(),
                None,
                "heartbeat_create",
                heartbeat.clone(),
            )
            .await,
    );

    let conversation = ConversationId::generate();
    let child = scoped
        .agent(json!({BOT_SCOPE_LABEL:scoped.workspace,BOT_CONVERSATION_LABEL:conversation}))
        .await;
    let created = scoped
        .call(
            &scoped.subtree_credential(&child),
            None,
            "heartbeat_create",
            heartbeat.clone(),
        )
        .await;
    let stored = scoped.stored(&schedule_id(&created)).await;
    assert_eq!(stored["botScope"], scoped.workspace.as_str(), "{stored}");
    assert_eq!(stored["botConversation"], json!(conversation), "{stored}");
    assert_eq!(stored["workspaceId"], scoped.workspace.as_str());
    assert_eq!(
        stored["target"],
        json!({"type":"agent","agentId":child}),
        "{stored}"
    );

    // A scope-labelled Agent placed outside the grant cannot schedule itself there.
    let stray = scoped
        .agent_in(
            &scoped.other,
            &scoped.other_cwd,
            json!({BOT_SCOPE_LABEL:scoped.workspace}),
            None,
        )
        .await;
    forbidden(
        &scoped
            .call(
                &scoped.subtree_credential(&stray),
                None,
                "heartbeat_create",
                heartbeat,
            )
            .await,
    );
    scoped.stop().await;
}
