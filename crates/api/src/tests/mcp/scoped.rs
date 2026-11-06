//! Bot and subtree credentials on the Agent MCP route: recognition, caller context and groups.

use std::collections::BTreeSet;

use domain::bot::permissions::{BotPermissions, ToolGroup};

use super::*;

mod confinement;
mod support;

use support::{BOT_SESSION, Scoped, codex_modes, rejected, succeeded};

#[tokio::test]
async fn scoped_credential_lists_only_granted_tools() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let host = scoped.tools(TOKEN).await;
    let bot = scoped.tools(&scoped.bot_credential()).await;
    for granted in [
        "create_agent",
        "list_schedules",
        "cancel_agent",
        "list_agents",
    ] {
        assert!(bot.contains(granted), "{granted}: {bot:?}");
    }
    assert!(host.contains("respond_to_permission"));
    for withheld in [
        "respond_to_permission",
        "archive_workspace",
        "rename_workspace",
        "list_workspaces",
        "create_terminal",
    ] {
        assert!(!bot.contains(withheld), "{withheld}");
    }
    let defaults = BotPermissions::default();
    let expected: BTreeSet<_> = host
        .iter()
        .filter(|tool| defaults.allows(tool, true))
        .cloned()
        .collect();
    assert_eq!(bot, expected);
    scoped.stop().await;
}

#[tokio::test]
async fn bot_calls_with_session_header_resolve_root_context() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let credential = scoped.bot_credential();
    let header = Some(BOT_SESSION);
    let schedules = scoped
        .call(&credential, header, "list_schedules", json!({}))
        .await;
    assert!(succeeded(&schedules), "{schedules}");
    let provider = scoped
        .call(
            &credential,
            header,
            "inspect_provider",
            json!({"provider":"codex"}),
        )
        .await;
    assert!(succeeded(&provider), "{provider}");
    let other = scoped.agent(json!({})).await;
    rejected(
        &scoped
            .call(&credential, Some(&other), "list_schedules", json!({}))
            .await,
        "Caller does not match credential",
    );
    scoped.stop().await;
}

#[tokio::test]
async fn permission_change_applies_on_next_request() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto", "full-access"])).await;
    let child = scoped.subtree_agent().await;
    let credential = scoped.subtree_credential(&child);
    let create = |mode: Option<&str>| {
        let mut arguments = json!({"provider":"codex/offline-model","title":"Grandchild",
            "initialPrompt":"work"});
        if let Some(mode) = mode {
            arguments["settings"] = json!({"modeId":mode});
        }
        arguments
    };
    let full = scoped
        .call(
            &credential,
            None,
            "create_agent",
            create(Some("full-access")),
        )
        .await;
    assert!(succeeded(&full), "{full}");
    scoped
        .permissions(json!({"groups":["observe","stop"],"modes":{"codex":["auto","full-access"]}}))
        .await;
    rejected(
        &scoped
            .call(&credential, None, "create_agent", create(None))
            .await,
        "Unknown or unavailable tool",
    );
    scoped
        .permissions(codex_modes(&["auto", "full-access"]))
        .await;
    let restored = scoped
        .call(&credential, None, "create_agent", create(None))
        .await;
    assert!(succeeded(&restored), "{restored}");
    scoped.permissions(codex_modes(&["auto"])).await;
    let capped = scoped
        .call(
            &credential,
            None,
            "create_agent",
            create(Some("full-access")),
        )
        .await;
    assert_eq!(capped["result"]["isError"], true, "{capped}");
    assert_eq!(
        capped["result"]["content"][0]["text"],
        model::ErrorCode::Forbidden.message()
    );
    scoped.stop().await;
}

#[tokio::test]
async fn shared_token_rejected_for_subtree_caller() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let child = scoped.subtree_agent().await;
    let external = scoped.agent(json!({})).await;
    for token in [AGENT_MCP_TOKEN, TOKEN] {
        rejected(
            &scoped
                .call(token, Some(&child), "list_agents", json!({}))
                .await,
            "Bot-scoped agents must use their own credential",
        );
        let listed = scoped.call(token, None, "list_agents", json!({})).await;
        assert!(succeeded(&listed), "{listed}");
        let inherited = scoped
            .call(token, Some(&external), "list_agents", json!({}))
            .await;
        assert!(succeeded(&inherited), "{inherited}");
    }
    scoped.stop().await;
}

#[tokio::test]
async fn caller_header_must_match_scoped_credential() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let child = scoped.subtree_agent().await;
    let other = scoped.subtree_agent().await;
    let credential = scoped.subtree_credential(&child);
    rejected(
        &scoped
            .call(&credential, Some(&other), "list_agents", json!({}))
            .await,
        "Caller does not match credential",
    );
    let created = scoped
        .call(
            &credential,
            None,
            "create_agent",
            json!({"provider":"codex/offline-model","title":"Grandchild","initialPrompt":"work"}),
        )
        .await;
    assert!(succeeded(&created), "{created}");
    let grandchild = created["result"]["structuredContent"]["agent"]["id"].clone();
    let status = scoped
        .call(
            TOKEN,
            None,
            "get_agent_status",
            json!({"agentId":grandchild}),
        )
        .await;
    let snapshot = &status["result"]["structuredContent"]["snapshot"];
    assert_eq!(snapshot["labels"]["paseo.parent-agent-id"], child);
    assert_eq!(snapshot["workspaceId"], scoped.workspace);
    scoped.stop().await;
}

#[tokio::test]
async fn scoped_credential_is_limited_to_the_mcp_route() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let credential = scoped.bot_credential();
    let subtree = scoped.subtree_credential(&scoped.subtree_agent().await);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for path in ["/v1/server/info", "/v1/ws"] {
        let status = client
            .get(scoped.fixture.url(path))
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(scoped.listing_status(&credential).await, StatusCode::OK);
    scoped.bot("bot.pause.request").await;
    assert_eq!(
        scoped.listing_status(&credential).await,
        StatusCode::UNAUTHORIZED
    );
    scoped.bot("bot.resume.request").await;
    assert_eq!(
        scoped.listing_status(&credential).await,
        StatusCode::UNAUTHORIZED
    );
    let reissued = scoped.bot_credential();
    assert_eq!(scoped.listing_status(&reissued).await, StatusCode::OK);
    scoped.bot("bot.remove.request").await;
    assert_eq!(
        scoped.listing_status(&reissued).await,
        StatusCode::UNAUTHORIZED
    );
    let observe = BotPermissions::observe_only();
    let host = scoped.tools(TOKEN).await;
    let remaining = scoped.tools(&subtree).await;
    assert!(!remaining.is_empty());
    assert!(remaining.iter().all(|tool| observe.allows(tool, false)));
    assert_eq!(
        remaining,
        host.iter()
            .filter(|tool| domain::bot::permissions::tool_access(tool)
                == domain::bot::permissions::ToolAccess::Group(ToolGroup::Observe))
            .cloned()
            .collect()
    );
    scoped.setup(Value::Null).await;
    assert_eq!(
        scoped.listing_status(&reissued).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        scoped.listing_status(&scoped.bot_credential()).await,
        StatusCode::OK
    );
    scoped.stop().await;
}
