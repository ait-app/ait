use super::*;

#[test]
fn metadata_follows_service_installation() {
    let specs: Vec<_> = implemented_methods().collect();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].kind, model::methods::InboundKind::Request);
    assert_eq!(specs[1].kind, model::methods::InboundKind::Response);
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), specs);
    assert_eq!(installed_methods(false).count(), 0);
}
