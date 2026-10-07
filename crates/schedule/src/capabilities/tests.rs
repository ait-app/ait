use super::*;

#[test]
fn metadata_follows_service_installation() {
    let specs: Vec<_> = implemented_methods().collect();
    assert_eq!(specs.len(), 9);
    assert!(
        specs
            .iter()
            .all(|spec| spec.kind == model::methods::InboundKind::Request)
    );
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), specs);
    assert_eq!(installed_methods(false).count(), 0);
}
