use std::collections::BTreeSet;

use super::*;

#[test]
fn empty_host_keeps_only_builtin_metadata_methods() {
    assert_eq!(features(&Services::default()), ["ait-rust-single-v1"]);
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
    let registered = crate::registered_capabilities(&methods);
    assert_eq!(registered.len(), 174);
    assert!(registered.iter().any(|method| method == "terminal.input"));
    assert!(!methods.iter().any(|method| method == "terminal.input"));
}

#[test]
fn merged_groups_have_one_owner_per_method_and_keep_placeholders_separate() {
    let methods: Vec<_> = implemented_methods().collect();
    let unique: BTreeSet<_> = methods.iter().copied().collect();
    assert_eq!(methods.len(), unique.len());
    assert_eq!(methods.len(), 179);
    assert!(unique.contains("schedule.list.request"));
    assert!(!unique.contains("server.status.unsubscribe"));
    let registered =
        crate::registered_capabilities(&methods.into_iter().map(str::to_owned).collect::<Vec<_>>());
    assert_eq!(registered.len(), 180);
}

#[test]
fn request_shapes_match_paseo_and_keep_only_dotted_methods() {
    assert_eq!(::filesystem::protocol::files::CAPABILITIES.len(), 11);
    assert!(
        ::filesystem::protocol::files::CAPABILITIES
            .iter()
            .all(|name| {
                protocol::methods::PASEO_METHODS
                    .iter()
                    .any(|method| method.canonical_name == *name)
            })
    );
}

#[test]
fn skill_methods_are_owned_by_filesystem_and_remain_canonical_requests() {
    for method in ::filesystem::protocol::skills::METHODS {
        let spec = protocol::methods::PASEO_METHODS
            .iter()
            .find(|spec| spec.canonical_name == *method)
            .unwrap();
        assert_eq!(spec.group, protocol::methods::MethodGroup::Skills);
        assert_eq!(spec.kind, protocol::methods::InboundKind::Request);
    }
}

#[test]
fn baseline_methods_and_heartbeat_keep_their_shared_contracts() {
    assert_eq!(
        protocol::CAPABILITIES,
        ::metadata::protocol::server::CAPABILITIES
    );
    let heartbeat =
        protocol::methods::by_canonical_name(::metadata::protocol::server::HEARTBEAT_METHOD)
            .unwrap();
    assert_eq!(heartbeat.kind, protocol::methods::InboundKind::Event);
    assert_eq!(heartbeat.group, protocol::methods::MethodGroup::Session);
}
