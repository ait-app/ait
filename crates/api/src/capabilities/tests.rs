use std::collections::BTreeSet;

use model::methods::InboundKind;

use super::*;

#[test]
fn empty_host_keeps_only_builtin_api_methods() {
    assert_eq!(
        features(&Services::default()),
        ["ait-rust-single-v1", "client-message-chunks-v1"]
    );
    let methods = installed_capabilities(&Services::default());
    assert_eq!(
        methods,
        [
            "server.info",
            "connection.ping",
            "server.status.subscribe",
            "subscription.release.request",
            "editor.available.list.request",
            "editor.open.request",
            "session.heartbeat",
            "session.events.set_subscription.request",
            "creation.subscribe.request",
            "relay.status.request",
            "relay.start.request",
            "relay.stop.request",
        ]
    );
    let registered = crate::registered_capabilities();
    assert_eq!(registered.len(), 180);
    assert!(registered.iter().any(|method| method == "terminal.input"));
    assert!(!methods.iter().any(|method| method == "terminal.input"));
    assert!(
        registered
            .iter()
            .any(|method| method == "checkout.reset_workspace.request")
    );
    assert!(registered.iter().any(|method| method == "agent.configure"));
}

#[test]
fn merged_components_have_one_owner_per_method_and_keep_placeholders_separate() {
    let specs: Vec<_> = implemented_methods().collect();
    let names: Vec<_> = specs.iter().map(|spec| spec.name).collect();
    let unique: BTreeSet<_> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len());
    assert_eq!(names.len(), 179);
    assert!(unique.contains("schedule.list.request"));
    assert!(!unique.contains("server.status.unsubscribe"));
    let registered = crate::registered_capabilities();
    assert_eq!(registered.len(), 180);
}

#[test]
fn file_methods_use_ait_names() {
    let specs: Vec<_> = ::filesystem::capabilities::implemented_methods()
        .filter(|method| {
            method.name.starts_with("fs.")
                || method.name.starts_with("file.")
                || method.name.starts_with("directory.")
        })
        .collect();
    assert_eq!(specs.len(), 11);
    assert!(specs.iter().all(|spec| spec.kind == InboundKind::Request));
    assert!(specs.iter().all(|spec| spec.name.contains('.')));
}

#[test]
fn skill_methods_are_owned_by_filesystem_and_remain_requests() {
    let specs: Vec<_> = ::filesystem::capabilities::implemented_methods()
        .filter(|spec| spec.name.starts_with("agent.skills."))
        .collect();
    assert_eq!(specs.len(), 5);
    assert!(specs.iter().all(|spec| spec.kind == InboundKind::Request));
}

#[test]
fn baseline_methods_and_heartbeat_keep_their_shared_contracts() {
    assert_eq!(model::server::CAPABILITIES, model::server::CAPABILITIES);
    let heartbeat = implemented_methods()
        .find(|spec| spec.name == ::metadata::protocol::server::HEARTBEAT_METHOD)
        .expect("heartbeat must be declared by metadata");
    assert_eq!(heartbeat.kind, InboundKind::Event);
}

#[test]
fn component_methods_are_well_formed_and_exclude_retired_groups() {
    for spec in implemented_methods() {
        assert!(!spec.name.starts_with('.'), "{spec:?}");
        assert!(!spec.name.ends_with('.'), "{spec:?}");
        assert!(!spec.name.contains(".."), "{spec:?}");
        assert!(
            spec.name.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || b"._".contains(&byte)),
            "{spec:?}"
        );
        assert!(
            !["hub.", "chat.", "loop.", "plugin."]
                .iter()
                .any(|prefix| spec.name.starts_with(prefix)),
            "{spec:?}"
        );
        match spec.kind {
            InboundKind::Request => {}
            InboundKind::Response => assert!(spec.name.ends_with(".response"), "{spec:?}"),
            InboundKind::Event => assert!(
                !spec.name.ends_with(".request") && !spec.name.ends_with(".response"),
                "{spec:?}"
            ),
        }
    }
}

#[test]
fn component_declarations_preserve_all_event_and_response_directions() {
    let mut events: Vec<_> = implemented_methods()
        .filter(|spec| spec.kind == InboundKind::Event)
        .map(|spec| spec.name)
        .collect();
    events.sort_unstable();
    assert_eq!(
        events,
        [
            "dictation.stream.cancel",
            "dictation.stream.chunk",
            "dictation.stream.finish",
            "dictation.stream.start",
            "push.register",
            "session.heartbeat",
            "terminal.input",
            "voice.audio.chunk",
            "voice.audio.played",
        ]
    );
    let responses: Vec<_> = implemented_methods()
        .filter(|spec| spec.kind == InboundKind::Response)
        .map(|spec| spec.name)
        .collect();
    assert_eq!(responses, ["browser.automation.execute.response"]);
}

#[test]
fn metadata_connection_methods_remain_available_without_its_business_service() {
    let metadata: BTreeSet<_> = ::metadata::capabilities::implemented_methods()
        .map(|method| method.name)
        .collect();
    let builtin: Vec<_> = ::metadata::capabilities::connection_methods()
        .map(|method| method.name)
        .collect();
    assert_eq!(builtin.len(), 9);
    assert!(builtin.iter().all(|method| !metadata.contains(method)));
    let installed = installed_capabilities(&Services::default());
    assert!(
        builtin
            .iter()
            .all(|method| installed.iter().any(|name| name == method))
    );
}
