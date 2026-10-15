use super::*;

#[test]
fn metadata_follows_service_installation() {
    let specs: Vec<_> = implemented_methods().collect();
    assert_eq!(specs.len(), 5);
    assert_eq!(specs[0].kind, model::methods::InboundKind::Request);
    assert_eq!(specs[1].kind, model::methods::InboundKind::Response);
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), specs);
    assert_eq!(installed_methods(false).count(), 0);
}

#[test]
fn remote_workspace_browser_can_negotiate_loopback_forwarding() {
    let methods: Vec<_> = implemented_methods().map(|spec| spec.name).collect();
    for name in [
        "browser.tunnel.open.request",
        "browser.tunnel.read.request",
        "browser.tunnel.write.request",
    ] {
        assert!(
            methods.contains(&name),
            "remote browser cannot forward {name}"
        );
    }
}
