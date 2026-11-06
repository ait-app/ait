use std::collections::{BTreeMap, BTreeSet};

use domain::BotId;
use domain::bot::permissions::ToolGroup;

use super::*;

const ROOT: &str = "wks_root";

fn scope(kind: CallerKind, permissions: BotPermissions) -> Scope {
    Scope {
        caller: Caller {
            root: ROOT.to_owned(),
            kind,
        },
        grant: Grant {
            permissions,
            workspaces: BTreeSet::from([ROOT.to_owned()]),
        },
    }
}

fn bot() -> CallerKind {
    CallerKind::Bot {
        bot_id: BotId::generate(),
        session_id: "session".to_owned(),
    }
}

fn subtree() -> CallerKind {
    CallerKind::Subtree {
        agent_id: "child".to_owned(),
    }
}

fn capped(modes: &[&str]) -> BotPermissions {
    BotPermissions {
        groups: BTreeSet::from([ToolGroup::Observe, ToolGroup::Agents]),
        modes: BTreeMap::from([(
            "codex".to_owned(),
            modes.iter().map(|mode| (*mode).to_owned()).collect(),
        )]),
    }
}

#[test]
fn bot_only_tools_are_granted_to_bot_sessions_but_never_to_subtrees() {
    let bot = scope(bot(), BotPermissions::default());
    let subtree = scope(subtree(), BotPermissions::default());
    for granted in [&bot, &subtree] {
        assert!(granted.allows("list_agents"));
        assert!(granted.allows("create_agent"));
        assert!(!granted.allows("create_terminal"));
        assert!(!granted.allows("respond_to_permission"));
        assert!(!granted.allows("future_tool"));
    }
    assert!(bot.allows("bot_memory_read"));
    assert!(!subtree.allows("bot_memory_read"));
}

#[test]
fn created_agent_modes_are_capped_and_defaulted() {
    let permissions = capped(&["auto", "full-access"]);
    let mut omitted = json!({"provider":"codex/model","title":"Task"});
    cap_mode(&permissions, &mut omitted).unwrap();
    assert_eq!(omitted["settings"]["modeId"], "auto");
    let mut explicit = json!({"provider":"codex/model","settings":{"modeId":"full-access"}});
    cap_mode(&permissions, &mut explicit).unwrap();
    assert_eq!(explicit["settings"]["modeId"], "full-access");
    for (permissions, provider, mode) in [
        (capped(&["auto"]), "codex/model", "full-access"),
        (capped(&["auto"]), "claude/model", "auto"),
        (BotPermissions::default(), "codex/model", "auto"),
    ] {
        let mut arguments = json!({"provider":provider,"settings":{"modeId":mode}});
        assert_eq!(
            cap_mode(&permissions, &mut arguments),
            Err(ErrorCode::Forbidden),
            "{provider} {mode}"
        );
    }
    assert_eq!(
        cap_mode(&permissions, &mut json!({"provider":"codex"})),
        Err(ErrorCode::InvalidMessage)
    );
}

const WORKTREE: &str = "wks_worktree";

fn capped_scope(kind: CallerKind) -> Scope {
    let mut scope = scope(kind, capped(&["auto", "read-only"]));
    scope.grant.workspaces.insert(WORKTREE.to_owned());
    scope
}

fn codex_agent(id: &str, workspace: &str) -> Value {
    json!({"id":id,"provider":"codex","workspaceId":workspace})
}

#[test]
fn bot_created_agents_are_root_level_and_labeled() {
    let mut arguments = json!({"provider":"codex/gpt","title":"Fix","initialPrompt":"go"});
    rewrite_create_agent(&capped_scope(bot()), None, &mut arguments).unwrap();
    assert_eq!(arguments["workspaceId"], ROOT);
    assert_eq!(arguments["labels"][BOT_SCOPE_LABEL], ROOT);
    assert_eq!(arguments["settings"]["modeId"], "auto");
    assert!(arguments.get("callerAgentId").is_none());

    let mut inherited = json!({"provider":"codex/gpt","labels":{"topic":"x"}});
    rewrite_create_agent(&capped_scope(subtree()), Some(WORKTREE), &mut inherited).unwrap();
    assert_eq!(inherited["workspaceId"], WORKTREE);
    assert_eq!(
        inherited["labels"],
        json!({"topic":"x",BOT_SCOPE_LABEL:ROOT})
    );

    let mut explicit = json!({"provider":"codex/gpt","workspaceId":WORKTREE});
    rewrite_create_agent(&capped_scope(bot()), None, &mut explicit).unwrap();
    assert_eq!(explicit["workspaceId"], WORKTREE);
    let mut outside = json!({"provider":"codex/gpt","workspaceId":"wks_other"});
    assert_eq!(
        rewrite_create_agent(&capped_scope(bot()), None, &mut outside),
        Err(ErrorCode::Forbidden)
    );
}

#[test]
fn mode_cap_covers_every_execution_entry() {
    let bot = capped_scope(bot());
    for arguments in [
        json!({"provider":"codex/gpt","settings":{"modeId":"full-access"}}),
        json!({"provider":"codex/gpt","settings":{"features":{"permissionMode":"bypassPermissions"}}}),
        json!({"provider":"codex/gpt","settings":{"features":{"fast_mode":true,"plan_mode":true}}}),
        json!({"provider":"claude/opus"}),
    ] {
        let mut rewritten = arguments.clone();
        assert_eq!(
            rewrite_create_agent(&bot, None, &mut rewritten),
            Err(ErrorCode::Forbidden),
            "{arguments}"
        );
    }
    let mut fast = json!({"provider":"codex/gpt","settings":{"features":{"fast_mode":true}}});
    rewrite_create_agent(&bot, None, &mut fast).unwrap();

    let target = codex_agent("agent", ROOT);
    let claude = json!({"id":"agent","provider":"claude","workspaceId":ROOT});
    for (agent, arguments) in [
        (
            &target,
            json!({"agentId":"agent","settings":{"modeId":"full-access"}}),
        ),
        (
            &target,
            json!({"agentId":"agent","settings":{"features":{"plan_mode":true}}}),
        ),
        (
            &claude,
            json!({"agentId":"agent","settings":{"modeId":"auto"}}),
        ),
    ] {
        assert_eq!(
            cap_update_agent(&bot, agent, &arguments),
            Err(ErrorCode::Forbidden),
            "{arguments}"
        );
    }
    let allowed = json!({"agentId":"agent","settings":{"modeId":"read-only",
        "features":{"fast_mode":false}},"name":"Renamed"});
    cap_update_agent(&bot, &target, &allowed).unwrap();
    // A pure display change never consults the target's provider cap.
    cap_update_agent(&bot, &claude, &json!({"agentId":"agent","name":"Renamed"})).unwrap();

    assert_eq!(
        cap_agent_mode(
            &bot,
            &target,
            &json!({"agentId":"agent","modeId":"full-access"})
        ),
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        cap_agent_mode(&bot, &claude, &json!({"agentId":"agent","modeId":"auto"})),
        Err(ErrorCode::Forbidden)
    );
    cap_agent_mode(
        &bot,
        &target,
        &json!({"agentId":"agent","modeId":"read-only"}),
    )
    .unwrap();
}

#[test]
fn unlisted_tools_reserved_labels_and_cwd_are_rejected() {
    for tool in [
        "rename_workspace",
        "archive_workspace",
        "list_workspaces",
        "respond_to_permission",
        "future_tool",
    ] {
        assert_eq!(admit(tool, &json!({})), Err(ErrorCode::Forbidden), "{tool}");
    }
    for tool in [
        "list_models",
        "inspect_provider",
        "create_terminal",
        "create_schedule",
    ] {
        assert_eq!(
            admit(tool, &json!({"cwd":"/tmp"})),
            Err(ErrorCode::Forbidden),
            "{tool}"
        );
        assert!(admit(tool, &json!({})).is_ok(), "{tool}");
    }
    // Every tool a Bot grant can ever expose is registered explicitly.
    for tool in domain::bot::permissions::grantable_tools() {
        assert!(admit(tool, &json!({})).is_ok(), "{tool}");
    }

    let reserved = json!({"ait.bot.x":"y"});
    assert_eq!(
        reserved_labels(&json!({"labels":reserved})),
        Err(ErrorCode::Forbidden)
    );
    reserved_labels(&json!({"labels":{"ait.botany":"y"}})).unwrap();
    reserved_labels(&json!({})).unwrap();
    let mut create = json!({"provider":"codex/gpt","labels":reserved});
    assert_eq!(
        rewrite_create_agent(&capped_scope(bot()), None, &mut create),
        Err(ErrorCode::Forbidden)
    );
    let update = json!({"agentId":"agent","labels":{BOT_SCOPE_LABEL:"wks_other"}});
    assert_eq!(
        cap_update_agent(
            &capped_scope(subtree()),
            &codex_agent("agent", ROOT),
            &update
        ),
        Err(ErrorCode::Forbidden)
    );
}

#[test]
fn target_ids_are_rewritten_to_canonical_ids() {
    let scope = capped_scope(bot());
    let mut arguments = json!({"agentId":"uuid","prompt":"go"});
    retarget(
        &scope,
        "agentId",
        Some(WORKTREE),
        "uuid-full",
        &mut arguments,
    )
    .unwrap();
    assert_eq!(arguments["agentId"], "uuid-full");
    let mut schedule = json!({"id":"sched"});
    retarget(&scope, "id", Some(ROOT), "sched-full", &mut schedule).unwrap();
    assert_eq!(schedule["id"], "sched-full");
    for workspace in [Some("wks_other"), None] {
        let mut outside = json!({"terminalId":"term"});
        assert_eq!(
            retarget(&scope, "terminalId", workspace, "term", &mut outside),
            Err(ErrorCode::Forbidden)
        );
        assert_eq!(outside["terminalId"], "term");
    }
}

#[test]
fn bot_worktrees_are_placed_in_the_root_project_before_the_cap_is_reached() {
    let root = json!({"id":ROOT,"projectId":"prj_root","workspaceDirectory":"/repo"});
    let scope = capped_scope(bot());
    let mut arguments = json!({"isolation":"worktree","path":"/elsewhere",
        "projectId":"prj_other","title":"Task"});
    rewrite_create_workspace(&scope, &root, &mut arguments).unwrap();
    assert_eq!(
        arguments,
        json!({"isolation":"worktree","path":"/repo","projectId":"prj_root","title":"Task"})
    );
    for arguments in [
        json!({"isolation":"local"}),
        json!({"isolation":"worktree","idempotencyKey":"replayed"}),
    ] {
        let mut rewritten = arguments.clone();
        assert_eq!(
            rewrite_create_workspace(&scope, &root, &mut rewritten),
            Err(ErrorCode::Forbidden),
            "{arguments}"
        );
    }
    let mut full = capped_scope(bot());
    full.grant.workspaces = (0..BOT_WORKSPACES)
        .map(|index| format!("wks_{index}"))
        .chain([ROOT.to_owned()])
        .collect();
    assert_eq!(
        rewrite_create_workspace(&full, &root, &mut json!({"isolation":"worktree"})),
        Err(ErrorCode::ResourceExhausted)
    );
    full.grant.workspaces.remove("wks_0");
    rewrite_create_workspace(&full, &root, &mut json!({"isolation":"worktree"})).unwrap();
}

#[test]
fn schedule_listing_keeps_only_scoped_workspaces() {
    let scope = capped_scope(bot());
    let mut result = json!({"schedules":[{"id":"a","workspaceId":ROOT},
        {"id":"b","workspaceId":"wks_other"},{"id":"c","workspaceId":WORKTREE},{"id":"d"}]});
    filter(&scope, "list_schedules", &mut result);
    assert_eq!(
        result,
        json!({"schedules":[{"id":"a","workspaceId":ROOT},{"id":"c","workspaceId":WORKTREE}]})
    );
    let mut untouched = json!({"schedules":[{"id":"b","workspaceId":"wks_other"}]});
    filter(&scope, "list_agents", &mut untouched);
    assert_eq!(untouched["schedules"][0]["id"], "b");
}
