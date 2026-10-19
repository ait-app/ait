use std::collections::BTreeSet;

use model::methods::InboundKind;

use super::*;

#[test]
fn installation_selects_the_entire_crate() {
    let declared: Vec<_> = implemented_methods().collect();
    let names: BTreeSet<_> = declared.iter().map(|spec| spec.name).collect();
    assert_eq!(declared.len(), 47);
    assert!(!names.contains("daemon.get_pairing_offer.request"));
    let workspace_target = declared
        .iter()
        .find(|method| method.name == "workspace.get.request")
        .expect("metadata must declare the target workspace request");
    assert_eq!(workspace_target.kind, InboundKind::Request);
    for name in [
        "workspace.setup.summary.request",
        "workspace.setup.subscribe.request",
    ] {
        let method = declared
            .iter()
            .find(|method| method.name == name)
            .expect("metadata must own scoped setup observation requests");
        assert_eq!(method.kind, InboundKind::Request);
    }
    assert_eq!(declared.len(), names.len());
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), declared);
    assert_eq!(installed_methods(false).count(), 0);
}

#[test]
fn connection_methods_are_separate_from_optional_business_installation() {
    let connections: Vec<_> = connection_methods().collect();
    let business: BTreeSet<_> = implemented_methods().map(|method| method.name).collect();
    let names: BTreeSet<_> = connections.iter().map(|method| method.name).collect();
    assert_eq!(connections.len(), 9);
    assert_eq!(connections.len(), names.len());
    assert!(names.is_disjoint(&business));
    assert_eq!(installed_methods(false).count(), 0);
    assert!(connections.iter().all(|method| {
        method.kind
            == if method.name == "session.heartbeat" {
                InboundKind::Event
            } else {
                InboundKind::Request
            }
    }));
}
