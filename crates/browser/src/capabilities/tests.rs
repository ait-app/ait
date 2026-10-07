use super::*;

#[test]
fn metadata_and_capability_names_follow_service_installation() {
    let specs: Vec<_> = implemented_methods().collect();
    let names: Vec<_> = specs.iter().map(|spec| spec.name).collect();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].kind, model::methods::InboundKind::Request);
    assert_eq!(specs[1].kind, model::methods::InboundKind::Response);
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), specs);
    assert_eq!(implemented_capabilities().collect::<Vec<_>>(), names);
    assert_eq!(installed_capabilities(true).collect::<Vec<_>>(), names);
    assert_eq!(installed_methods(false).count(), 0);
    assert_eq!(installed_capabilities(false).count(), 0);
}
