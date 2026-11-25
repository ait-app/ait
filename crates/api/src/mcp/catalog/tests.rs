use std::collections::BTreeSet;

use serde_json::json;

use super::{all, resolve};

#[test]
fn retired_names_resolve_to_their_tool_and_never_shadow_a_current_name() {
    let current: BTreeSet<_> = all().iter().map(|tool| tool.name.as_str()).collect();
    let mut retired = BTreeSet::new();
    for tool in all() {
        assert!(matches!(resolve(&tool.name), Some((found, None)) if found.name == tool.name));
        for alias in &tool.aliases {
            assert!(!current.contains(alias.name.as_str()), "{}", alias.name);
            assert!(retired.insert(alias.name.as_str()), "{}", alias.name);
            let (found, by) = resolve(&alias.name).unwrap();
            assert_eq!(found.name, tool.name);
            assert_eq!(by.unwrap().name, alias.name);
            for (old, new) in &alias.arguments {
                let properties = &tool.input_schema["properties"];
                assert!(properties.get(new).is_some(), "{}: {new}", alias.name);
                assert!(properties.get(old).is_none(), "{}: {old}", alias.name);
            }
        }
    }
    assert_eq!(retired.len(), 45);
    assert!(resolve("unknown_tool").is_none());
    assert_eq!(resolve("kill_agent").unwrap().0.name, "agent_archive");
    assert_eq!(resolve("report_progress").unwrap().0.name, "ticket_report");
}

#[test]
fn a_retired_name_rewrites_its_retired_arguments() {
    let (_, alias) = resolve("update_schedule").unwrap();
    let alias = alias.unwrap();
    let mut arguments = json!({"id":"sch_1","mode":"default","cron":"0 * * * *"});
    alias.rewrite(&mut arguments);
    assert_eq!(
        arguments,
        json!({"scheduleId":"sch_1","modeId":"default","cron":"0 * * * *"})
    );

    // A caller that already switched to the current argument keeps its value.
    let mut mixed = json!({"id":"old","scheduleId":"new"});
    alias.rewrite(&mut mixed);
    assert_eq!(mixed, json!({"scheduleId":"new"}));

    let mut not_an_object = json!([1]);
    alias.rewrite(&mut not_an_object);
    assert_eq!(not_an_object, json!([1]));

    let (_, unchanged) = resolve("create_agent").unwrap();
    let mut create = json!({"title":"t"});
    unchanged.unwrap().rewrite(&mut create);
    assert_eq!(create, json!({"title":"t"}));
}

#[test]
fn uses_of_a_retired_name_are_counted() {
    let (tool, alias) = resolve("list_terminals").unwrap();
    let alias = alias.unwrap();
    let before = alias.uses.load(std::sync::atomic::Ordering::Relaxed);
    alias.record(&tool.name);
    alias.record(&tool.name);
    assert_eq!(
        alias.uses.load(std::sync::atomic::Ordering::Relaxed),
        before + 2
    );
}

#[test]
fn exactly_the_thirteen_tools_are_marked_for_peers() {
    let marked: BTreeSet<_> = all()
        .iter()
        .filter(|tool| tool.peer)
        .map(|tool| tool.name.as_str())
        .collect();
    assert_eq!(
        marked,
        BTreeSet::from([
            "agent_create",
            "agent_prompt",
            "agent_get",
            "agent_activity_read",
            "agent_list",
            "agent_wait",
            "agent_cancel",
            "agent_archive",
            "workspace_create",
            "workspace_list",
            "provider_list",
            "provider_model_list",
            "profile_list",
        ])
    );
}

#[tokio::test]
async fn a_marked_tool_whose_methods_are_not_installed_is_not_offered_to_peers() {
    let api = crate::Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        "stable".into(),
        "instance".into(),
        "offline-test-token-at-least-32-characters".into(),
        crate::Services::default(),
    )
    .unwrap();
    let (tool, _) = resolve("agent_create").unwrap();
    assert!(tool.peer);
    assert!(
        !api.shared
            .info
            .implemented_capabilities
            .contains(&tool.methods[0])
    );

    assert!(super::for_peers(&api.shared, "agent_create").is_none());
}
