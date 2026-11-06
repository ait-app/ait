use std::collections::{BTreeMap, BTreeSet};

use super::{BotPermissions, PermissionError, TOOLS, ToolAccess, ToolGroup};
use super::{grantable_tools, tool_access};

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
    assert_eq!(TOOLS.len(), 43);
    for tool in [
        "respond_to_permission",
        "archive_workspace",
        "rename_workspace",
        "list_workspaces",
        "unknown",
    ] {
        assert_eq!(tool_access(tool), ToolAccess::HostOnly);
    }
    assert!(!grantable_tools().any(|tool| tool == "respond_to_permission"));
    assert!(grantable_tools().any(|tool| tool == "bot_memory_update"));
}

#[test]
fn observe_is_always_granted_and_bot_tools_only_for_bot() {
    let permissions = with_modes(BTreeMap::new()).normalized().unwrap();
    assert!(permissions.allows("list_agents", false));
    assert!(!permissions.allows("create_agent", true));
    assert!(permissions.allows("workspace_overview", true));
    assert!(!permissions.allows("workspace_overview", false));
    assert!(!BotPermissions::default().allows("create_terminal", true));
    assert!(BotPermissions::default().allows("create_schedule", false));
    assert!(!BotPermissions::default().allows("respond_to_permission", true));
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
    assert_eq!(permissions.mode("claude", None).unwrap(), "default");
    assert_eq!(permissions.mode("claude", Some("plan")).unwrap(), "plan");
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
