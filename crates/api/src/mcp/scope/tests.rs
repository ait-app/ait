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
