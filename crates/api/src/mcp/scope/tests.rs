use std::collections::{BTreeMap, BTreeSet};

use domain::bot::BOT_LABEL_PREFIX;
use domain::bot::permissions::ToolGroup;
use domain::{BotId, ConversationId};

use super::*;

const ROOT: &str = "wks_root";
/// Conversation of the Bot session built by [`bot`].
const CONVERSATION: &str = "33333333-3333-4333-8333-333333333333";

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
        conversation_id: CONVERSATION.parse().unwrap(),
        session_id: "session".to_owned(),
    }
}

fn subtree() -> CallerKind {
    CallerKind::Subtree {
        agent_id: "child".to_owned(),
        ticket: None,
    }
}

fn worker() -> CallerKind {
    CallerKind::Subtree {
        agent_id: "child".to_owned(),
        ticket: Some(domain::TicketId::generate()),
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
    let worker = scope(worker(), BotPermissions::default());
    assert!(worker.allows("create_agent"));
    assert!(!worker.allows("bot_tickets"));
    for tool in ["bot_tickets", "bot_ticket_update"] {
        assert!(bot.allows(tool), "{tool}");
        assert!(!subtree.allows(tool), "{tool}");
    }
    assert!(worker.allows("report_progress"));
    assert!(!subtree.allows("report_progress"));
    assert!(!bot.allows("report_progress"));
    for tool in ["bot_tickets", "bot_ticket_update", "report_progress"] {
        assert_eq!(admit(tool, &json!({})), Ok(Rule::Open), "{tool}");
    }
}

#[test]
fn script_run_reads_are_observe_tools_confined_to_scoped_workspaces() {
    let observe_only = BotPermissions {
        groups: BTreeSet::from([ToolGroup::Observe]),
        modes: BTreeMap::new(),
    };
    for kind in [bot(), subtree()] {
        let scope = scope(kind, observe_only.clone());
        for tool in ["get_workspace_script_run", "wait_for_workspace_script"] {
            assert!(scope.allows(tool), "{tool}");
            assert_eq!(
                admit(tool, &json!({"workspaceId":ROOT})),
                Ok(Rule::Workspace)
            );
            assert_eq!(
                admit(tool, &json!({"workspaceId":ROOT,"cwd":"/tmp"})),
                Err(ErrorCode::Forbidden)
            );
        }
        assert!(!scope.allows("start_workspace_script"));
        assert_eq!(scope.contains(Some(ROOT)), Ok(()));
        assert_eq!(scope.contains(Some("wks_other")), Err(ErrorCode::Forbidden));
    }
}

#[test]
fn created_agent_modes_are_capped_and_defaulted() {
    let permissions = capped(&["auto"]);
    let mut omitted = json!({"provider":"codex/model","title":"Task"});
    cap_mode(&permissions, &mut omitted).unwrap();
    assert_eq!(omitted["settings"]["modeId"], "auto");
    let mut explicit = json!({"provider":"codex/model","settings":{"modeId":"auto"}});
    cap_mode(&permissions, &mut explicit).unwrap();
    assert_eq!(explicit["settings"]["modeId"], "auto");
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
    let mut scope = scope(kind, capped(&["auto"]));
    scope.grant.workspaces.insert(WORKTREE.to_owned());
    scope
}

fn codex_agent(id: &str, workspace: &str) -> Value {
    json!({"id":id,"provider":"codex","workspaceId":workspace})
}

fn origin() -> Option<ConversationId> {
    CONVERSATION.parse().ok()
}

#[test]
fn bot_created_agents_are_root_level_and_labeled() {
    let mut arguments = json!({"provider":"codex/gpt","title":"Fix","initialPrompt":"go"});
    rewrite_create_agent(&capped_scope(bot()), None, origin(), &mut arguments).unwrap();
    assert_eq!(arguments["workspaceId"], ROOT);
    assert_eq!(arguments["labels"][BOT_SCOPE_LABEL], ROOT);
    assert_eq!(arguments["labels"][BOT_CONVERSATION_LABEL], CONVERSATION);
    assert_eq!(arguments["settings"]["modeId"], "auto");
    assert!(arguments.get("callerAgentId").is_none());

    let mut inherited = json!({"provider":"codex/gpt","labels":{"topic":"x"}});
    rewrite_create_agent(
        &capped_scope(subtree()),
        Some(WORKTREE),
        None,
        &mut inherited,
    )
    .unwrap();
    assert_eq!(inherited["workspaceId"], WORKTREE);
    assert_eq!(
        inherited["labels"],
        json!({"topic":"x",BOT_SCOPE_LABEL:ROOT})
    );

    let mut explicit = json!({"provider":"codex/gpt","workspaceId":WORKTREE});
    rewrite_create_agent(&capped_scope(bot()), None, origin(), &mut explicit).unwrap();
    assert_eq!(explicit["workspaceId"], WORKTREE);
    let mut outside = json!({"provider":"codex/gpt","workspaceId":"wks_other"});
    assert_eq!(
        rewrite_create_agent(&capped_scope(bot()), None, origin(), &mut outside),
        Err(ErrorCode::Forbidden)
    );
}

#[test]
fn the_origin_conversation_is_the_bot_session_or_the_subtree_callers_label() {
    let bot = capped_scope(bot());
    assert_eq!(bot.origin(None), origin());
    let labelled = json!({"id":"child","labels":{BOT_CONVERSATION_LABEL:CONVERSATION}});
    assert_eq!(bot.origin(Some(&json!({"labels":{}}))), origin());

    let subtree = capped_scope(subtree());
    assert_eq!(subtree.origin(Some(&labelled)), origin());
    for context in [
        json!({"id":"child","labels":{BOT_SCOPE_LABEL:ROOT}}),
        json!({"id":"child","labels":{BOT_CONVERSATION_LABEL:"not-a-conversation"}}),
        json!({"id":"child"}),
    ] {
        assert_eq!(subtree.origin(Some(&context)), None, "{context}");
    }
    assert_eq!(subtree.origin(None), None);

    let mut schedule = json!({"provider":"codex/model"});
    record_origin(subtree.origin(Some(&labelled)), &mut schedule);
    assert_eq!(schedule["botConversation"], CONVERSATION);
    let mut unlabelled = json!({"provider":"codex/model"});
    record_origin(None, &mut unlabelled);
    assert!(unlabelled.get("botConversation").is_none());
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
            rewrite_create_agent(&bot, None, None, &mut rewritten),
            Err(ErrorCode::Forbidden),
            "{arguments}"
        );
    }
    let mut fast = json!({"provider":"codex/gpt","settings":{"features":{"fast_mode":true}}});
    rewrite_create_agent(&bot, None, None, &mut fast).unwrap();

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
    let allowed = json!({"agentId":"agent","settings":{"modeId":"auto",
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
    cap_agent_mode(&bot, &target, &json!({"agentId":"agent","modeId":"auto"})).unwrap();
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
        reserved_labels(&json!({"labels":reserved}), BOT_LABEL_PREFIX),
        Err(ErrorCode::Forbidden)
    );
    let origin = json!({BOT_CONVERSATION_LABEL:CONVERSATION});
    assert_eq!(
        reserved_labels(&json!({"labels":origin}), BOT_LABEL_PREFIX),
        Err(ErrorCode::Forbidden)
    );
    reserved_labels(&json!({"labels":{"ait.botany":"y"}}), BOT_LABEL_PREFIX).unwrap();
    reserved_labels(&json!({}), BOT_LABEL_PREFIX).unwrap();
    let mut create = json!({"provider":"codex/gpt","labels":reserved});
    assert_eq!(
        rewrite_create_agent(&capped_scope(bot()), None, None, &mut create),
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

#[test]
fn schedule_patches_keep_the_effective_mode_within_the_cap() {
    let permissions = capped(&["auto"]);
    let task = |config: Value| json!({"target":{"type":"new-agent","config":config}});
    let unset = task(json!({"provider":"codex"}));
    let escalated = task(json!({"provider":"codex","modeId":"full-access"}));
    let mut bot_unset = unset.clone();
    bot_unset["botScope"] = json!(ROOT);
    // Only a Bot-created task runs a missing mode at the cap's default.
    assert_eq!(
        cap_schedule(&permissions, &unset),
        Err(Failure::Refused(UNSET_MODE))
    );
    assert_eq!(cap_schedule(&permissions, &bot_unset), Ok(()));
    assert_eq!(
        cap_schedule(&permissions, &task(json!({"provider":"claude"}))),
        Err(Failure::Code(ErrorCode::Forbidden))
    );
    assert_eq!(
        cap_schedule(&permissions, &escalated),
        Err(Failure::Code(ErrorCode::Forbidden))
    );
    let heartbeat = json!({"target":{"type":"agent","agentId":"agent"}});
    assert_eq!(cap_schedule(&permissions, &heartbeat), Ok(()));

    for (patch, checked, mode) in [
        (json!({"mode":null}), true, json!("auto")),
        (json!({"mode":"auto"}), true, json!("auto")),
        (json!({"provider":"codex/next"}), true, json!("auto")),
        (json!({"model":"next"}), false, Value::Null),
        (json!({"prompt":"changed"}), false, Value::Null),
    ] {
        let mut patched = patch.clone();
        assert_eq!(
            cap_schedule_update(&permissions, &escalated, &mut patched),
            Ok(checked),
            "{patch}"
        );
        assert_eq!(patched["mode"], mode, "{patch}");
    }
    for patch in [
        json!({"mode":"full-access"}),
        json!({"mode":"read-only"}),
        json!({"provider":"claude"}),
        json!({"provider":"claude/model","mode":"auto"}),
    ] {
        let mut patched = patch.clone();
        assert_eq!(
            cap_schedule_update(&permissions, &unset, &mut patched),
            Err(ErrorCode::Forbidden),
            "{patch}"
        );
    }
}

#[test]
fn scoped_schedules_record_the_bot_scope_and_heartbeats_need_an_agent_caller() {
    let bot = capped_scope(bot());
    let mut arguments = json!({"provider":"codex/model"});
    rewrite_create_schedule(&bot, None, &mut arguments).unwrap();
    assert_eq!(arguments["modeId"], "auto");
    assert_eq!(arguments["botScope"], ROOT);
    let mut missing = json!({});
    assert_eq!(
        rewrite_create_schedule(&bot, None, &mut missing),
        Err(ErrorCode::InvalidMessage)
    );
    let mut unlisted = json!({"provider":"claude"});
    assert_eq!(
        rewrite_create_schedule(&bot, None, &mut unlisted),
        Err(ErrorCode::Forbidden)
    );

    let child = codex_agent("child", ROOT);
    assert_eq!(
        heartbeat_workspace(&bot, Some(&child)),
        Err(ErrorCode::Forbidden)
    );
    let subtree = capped_scope(subtree());
    assert_eq!(heartbeat_workspace(&subtree, Some(&child)), Ok(ROOT));
    assert_eq!(
        heartbeat_workspace(&subtree, Some(&codex_agent("child", "wks_other"))),
        Err(ErrorCode::Forbidden)
    );
    assert_eq!(
        heartbeat_workspace(&subtree, None),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn scoped_callers_cannot_write_any_ait_label() {
    for label in [
        "ait.guest.owner",
        "ait.botany",
        "ait.x",
        BOT_CONVERSATION_LABEL,
    ] {
        let labels = json!({ label: "y" });
        let mut create = json!({"provider":"codex/gpt","labels":labels});
        assert_eq!(
            rewrite_create_agent(&capped_scope(bot()), None, origin(), &mut create),
            Err(ErrorCode::Forbidden),
            "{label}"
        );
        let update = json!({"agentId":"agent","labels":labels});
        assert_eq!(
            cap_update_agent(
                &capped_scope(subtree()),
                &codex_agent("agent", ROOT),
                &update
            ),
            Err(ErrorCode::Forbidden),
            "{label}"
        );
    }
    let mut create = json!({"provider":"codex/gpt","labels":{"team":"ait"}});
    rewrite_create_agent(&capped_scope(bot()), None, origin(), &mut create).unwrap();
}

#[test]
fn separate_model_selection_obeys_the_same_mode_cap() {
    let scope = capped_scope(bot());
    let mut separate = json!({"provider":"codex","model":"namespace/model"});
    let mut combined = json!({"provider":"codex/namespace/model"});
    rewrite_create_agent(&scope, None, origin(), &mut separate).unwrap();
    rewrite_create_agent(&scope, None, origin(), &mut combined).unwrap();
    assert_eq!(separate["settings"], combined["settings"]);
    for mut arguments in [
        json!({"provider":"claude","model":"namespace/model"}),
        json!({"provider":"codex","model":"namespace/model","settings":{"modeId":"full-access"}}),
    ] {
        assert_eq!(
            rewrite_create_agent(&scope, None, origin(), &mut arguments),
            Err(ErrorCode::Forbidden)
        );
    }
}
