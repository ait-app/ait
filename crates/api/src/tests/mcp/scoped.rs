//! Bot and subtree credentials on the Agent MCP route: recognition, caller context and groups.

use std::collections::BTreeSet;

use domain::bot::permissions::{BotPermissions, ToolGroup};

use super::*;

mod confinement;
mod schedules;
mod support;

use support::{BOT_SESSION, Scoped, codex_modes, rejected, structured, succeeded};

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
        "workspace_overview",
        "bot_memory_read",
        "bot_memory_update",
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
    let bot_only = ["bot_memory_read", "bot_memory_update", "workspace_overview"];
    assert!(bot_only.iter().all(|tool| !host.contains(*tool)));
    let expected: BTreeSet<_> = host
        .iter()
        .filter(|tool| defaults.allows(tool, true))
        .cloned()
        .chain(bot_only.map(str::to_owned))
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
    let configured = scoped
        .call(&credential, None, "create_agent", create(Some("auto")))
        .await;
    assert!(succeeded(&configured), "{configured}");
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

#[tokio::test]
async fn bot_memory_tools_are_revision_checked_and_bot_only() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let credential = scoped.bot_credential();
    let read = scoped
        .call(&credential, None, "bot_memory_read", json!({}))
        .await;
    let first = structured(&read)["revision"].as_str().unwrap().to_owned();
    assert_eq!(structured(&read)["text"], "");
    let update = |text: &str, expected: &str| json!({"text":text,"expected":expected});
    let saved = scoped
        .call(
            &credential,
            None,
            "bot_memory_update",
            update("PR titles in Chinese", &first),
        )
        .await;
    let second = structured(&saved)["revision"].as_str().unwrap().to_owned();
    assert_ne!(second, first);
    let stale = scoped
        .call(
            &credential,
            None,
            "bot_memory_update",
            update("lost", &first),
        )
        .await;
    assert_eq!(stale["result"]["isError"], true, "{stale}");
    assert!(
        stale["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("bot_memory_read"),
        "{stale}"
    );
    let reread = scoped
        .call(&credential, None, "bot_memory_read", json!({}))
        .await;
    assert_eq!(structured(&reread)["text"], "PR titles in Chinese");
    assert_eq!(structured(&reread)["revision"], second.as_str());
    let oversized = scoped
        .call(
            &credential,
            None,
            "bot_memory_update",
            update(&"x".repeat(32 * 1024 + 1), &second),
        )
        .await;
    rejected(&oversized, "Arguments do not match the tool input schema");
    let overview = scoped
        .call(&credential, None, "workspace_overview", json!({}))
        .await;
    let overview = structured(&overview);
    assert_eq!(overview["workspace"]["id"], scoped.workspace.as_str());
    assert_eq!(overview["workspace"]["branch"], "main");
    assert_eq!(overview["memory"]["revision"], second.as_str());
    assert_eq!(overview["memory"]["bytes"], 20);
    assert_eq!(overview["botAgents"], json!([]));
    assert!(overview["notifications"].is_null());
    let child = scoped.subtree_agent().await;
    let other = scoped
        .agent_in(&scoped.other, &scoped.other_cwd, json!({}), None)
        .await;
    let overview = scoped
        .call(&credential, None, "workspace_overview", json!({}))
        .await;
    let overview = structured(&overview);
    let listed: Vec<_> = overview["botAgents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, [child.as_str()]);
    assert!(!overview.to_string().contains(&other));
    let subtree = scoped.subtree_credential(&child);
    assert!(scoped.tools(TOKEN).await.is_disjoint(&bot_only_names()));
    assert!(scoped.tools(&subtree).await.is_disjoint(&bot_only_names()));
    for token in [TOKEN, subtree.as_str()] {
        for tool in bot_only_names() {
            rejected(
                &scoped.call(token, None, &tool, json!({})).await,
                "Unknown or unavailable tool",
            );
        }
    }
    scoped.stop().await;
}

fn bot_only_names() -> BTreeSet<String> {
    ["bot_memory_read", "bot_memory_update", "workspace_overview"]
        .map(str::to_owned)
        .into()
}
