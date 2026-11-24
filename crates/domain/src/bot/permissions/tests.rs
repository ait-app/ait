use std::collections::{BTreeMap, BTreeSet};

use super::{BotPermissions, Holder, PermissionError, TOOLS, ToolAccess, ToolGroup};
use super::{grantable_tools, tool_access, worker_tools};

fn with_modes(modes: BTreeMap<String, Vec<String>>) -> BotPermissions {
    BotPermissions {
        groups: BTreeSet::new(),
        modes,
    }
}

#[test]
fn every_tool_has_one_access_class_and_never_tools_are_host_only() {
    let names: BTreeSet<_> = TOOLS.iter().map(|(name, _)| *name).collect();
    assert_eq!(names.len(), TOOLS.len());
    for tool in [
        "permission_respond",
        "workspace_archive",
        "workspace_rename",
        "workspace_list",
        "workspace_create",
        "host_list",
        "unknown",
    ] {
        assert_eq!(tool_access(tool), ToolAccess::HostOnly);
    }
    // Listing the account's other Hosts is for host credentials only, by an explicit entry.
    assert!(TOOLS.contains(&("host_list", ToolAccess::HostOnly)));
    assert!(!grantable_tools().any(|tool| tool == "host_list"));
    assert!(!grantable_tools().any(|tool| tool == "permission_respond"));
    assert!(!grantable_tools().any(|tool| tool == "workspace_create"));
    assert!(grantable_tools().any(|tool| tool == "bot_memory_update"));
    assert!(grantable_tools().any(|tool| tool == "bot_ticket_update"));
    assert_eq!(tool_access("ticket_report"), ToolAccess::Worker);
    assert!(!grantable_tools().any(|tool| tool == "ticket_report"));
    assert!(grantable_tools().any(|tool| tool == "bot_read"));
    assert_eq!(tool_access("ticket_read"), ToolAccess::Worker);
    assert!(!grantable_tools().any(|tool| tool == "ticket_read"));
    assert_eq!(
        worker_tools().collect::<Vec<_>>(),
        ["ticket_report", "ticket_read"]
    );
}

#[test]
fn observe_is_always_granted_and_bot_tools_only_for_bot() {
    let permissions = with_modes(BTreeMap::new()).normalized().unwrap();
    assert!(permissions.allows("agent_list", Holder::Subtree));
    assert!(!permissions.allows("agent_create", Holder::Bot));
    assert!(permissions.allows("bot_overview", Holder::Bot));
    assert!(!permissions.allows("bot_overview", Holder::Subtree));
    assert!(!permissions.allows("bot_ticket_list", Holder::Worker));
    assert!(!BotPermissions::default().allows("terminal_create", Holder::Bot));
    assert!(BotPermissions::default().allows("schedule_create", Holder::Subtree));
    assert!(BotPermissions::default().allows("schedule_create", Holder::Worker));
    assert!(!BotPermissions::default().allows("permission_respond", Holder::Bot));
}

#[test]
fn worker_tools_are_only_for_subtree_agents_on_a_ticket() {
    let permissions = BotPermissions::observe_only();
    assert!(permissions.allows("ticket_report", Holder::Worker));
    assert!(!permissions.allows("ticket_report", Holder::Subtree));
    assert!(!permissions.allows("ticket_report", Holder::Bot));
    assert!(permissions.allows("ticket_read", Holder::Worker));
    assert!(!permissions.allows("ticket_read", Holder::Subtree));
    assert!(!permissions.allows("ticket_read", Holder::Bot));
    assert!(permissions.allows("bot_read", Holder::Bot));
    assert!(!permissions.allows("bot_read", Holder::Worker));
    assert!(!permissions.allows("bot_read", Holder::Subtree));
}

#[test]
fn retired_workspaces_group_is_dropped_and_grants_nothing() {
    let saved: BotPermissions =
        serde_json::from_value(serde_json::json!({"groups":["workspaces"],"modes":{}})).unwrap();
    assert!(!saved.allows("workspace_create", Holder::Bot));
    assert_eq!(
        saved.normalized().unwrap().groups,
        BTreeSet::from([ToolGroup::Observe])
    );
}

#[test]
fn stored_profile_with_several_modes_enforces_only_the_first() {
    let saved = with_modes(BTreeMap::from([(
        "claude".into(),
        vec!["default".into(), "plan".into()],
    )]));
    assert_eq!(
        saved.mode("claude", Some("plan")),
        Err(PermissionError::Mode)
    );
}

#[test]
fn observe_only_profile_grants_only_observe() {
    let permissions = BotPermissions::observe_only();
    assert_eq!(permissions.groups, BTreeSet::from([ToolGroup::Observe]));
    assert!(permissions.modes.is_empty());
}

#[test]
fn mode_cap_resolves_defaults_and_rejects_unlisted() {
    let permissions = BotPermissions {
        groups: BTreeSet::from([ToolGroup::Agents]),
        modes: BTreeMap::from([("claude".into(), vec!["default".into(), "plan".into()])]),
    }
    .normalized()
    .unwrap();
    assert_eq!(permissions.modes["claude"], vec!["default".to_owned()]);
    assert_eq!(permissions.mode("claude", None).unwrap(), "default");
    assert_eq!(
        permissions.mode("claude", Some("default")).unwrap(),
        "default"
    );
    assert_eq!(
        permissions.mode("claude", Some("plan")),
        Err(PermissionError::Mode)
    );
    assert_eq!(
        permissions.mode("claude", Some("bypassPermissions")),
        Err(PermissionError::Mode)
    );
    assert_eq!(
        permissions.mode("codex", None),
        Err(PermissionError::Provider)
    );
    assert_eq!(
        BotPermissions::default().mode("claude", None),
        Err(PermissionError::Provider)
    );
}

#[test]
fn invalid_profiles_are_rejected() {
    let long = "x".repeat(129);
    let modes = |provider: &str, list: Vec<String>| {
        with_modes(BTreeMap::from([(provider.to_owned(), list)])).normalized()
    };
    assert_eq!(modes("claude", vec![]), Err(PermissionError::Invalid));
    assert_eq!(
        modes("claude", vec!["a".into(), "a".into()]),
        Err(PermissionError::Invalid)
    );
    let seventeen = (0..17).map(|index| format!("m{index}")).collect();
    assert_eq!(modes("claude", seventeen), Err(PermissionError::Invalid));
    let sixteen: Vec<String> = (0..16).map(|index| format!("m{index}")).collect();
    assert!(modes("claude", sixteen).is_ok());
    let many = (0..33)
        .map(|index| (format!("p{index}"), vec!["default".to_owned()]))
        .collect();
    assert_eq!(with_modes(many).normalized(), Err(PermissionError::Invalid));
    for bad in [long.as_str(), "", "a\nb"] {
        assert_eq!(
            modes(bad, vec!["default".into()]),
            Err(PermissionError::Invalid)
        );
        assert_eq!(
            modes("claude", vec![bad.into()]),
            Err(PermissionError::Invalid)
        );
    }
}
