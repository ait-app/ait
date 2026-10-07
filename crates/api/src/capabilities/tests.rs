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
fn merged_components_have_one_owner_per_method_and_keep_placeholders_separate() {
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
    let methods: Vec<_> = ::filesystem::capabilities::implemented_capabilities().collect();
    let specs: Vec<_> = methods
        .iter()
        .filter_map(|method| protocol::methods::by_canonical_name(method))
        .collect();
    assert_eq!(
        specs
            .iter()
            .filter(|spec| spec.group == protocol::methods::MethodGroup::Files)
            .count(),
        11
    );
    assert!(methods.iter().all(|method| method.contains('.')));
}

#[test]
fn skill_methods_are_owned_by_filesystem_and_remain_canonical_requests() {
    let methods: Vec<_> = ::filesystem::capabilities::implemented_capabilities()
        .filter(|method| method.starts_with("agent.skills."))
        .collect();
    assert_eq!(methods.len(), 5);
    for method in methods {
        let spec = protocol::methods::by_canonical_name(method).unwrap();
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
