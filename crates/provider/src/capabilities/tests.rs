use std::collections::BTreeSet;

use super::*;

#[test]
fn installation_selects_the_entire_crate() {
    let declared: Vec<_> = implemented_methods().collect();
    let names: BTreeSet<_> = declared.iter().map(|spec| spec.name).collect();
    assert_eq!(declared.len(), 55);
    assert_eq!(declared.len(), names.len());
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), declared);
    assert_eq!(installed_methods(false).count(), 0);
    for method in [
        "agent.commands.complete.request",
        "agent.native_action.request",
    ] {
        let matches: Vec<_> = declared.iter().filter(|spec| spec.name == method).collect();
        assert_eq!(matches.len(), 1, "{method} must have one provider owner");
        assert_eq!(matches[0].kind, model::methods::InboundKind::Request);
    }
}
