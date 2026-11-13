use std::collections::BTreeMap;

use model::methods::InboundKind;

use super::*;

#[test]
fn installation_selects_the_entire_crate() {
    let declared: Vec<_> = implemented_methods().collect();
    let declarations: BTreeMap<_, _> = declared.iter().map(|spec| (spec.name, spec.kind)).collect();
    assert_eq!(
        declared.len(),
        declarations.len(),
        "duplicate method declaration"
    );
    let installed: Vec<_> = installed_methods(true).collect();
    let installation: BTreeMap<_, _> = installed
        .iter()
        .map(|spec| (spec.name, spec.kind))
        .collect();
    assert_eq!(
        installed.len(),
        installation.len(),
        "duplicate installed method"
    );
    assert_eq!(installed, declared);
    assert!(installed_methods(false).next().is_none());

    for method in [
        "agent.side_question.ask.request",
        "agent.side_question.cancel.request",
        "agent.side_question.history.request",
        "agent.side_question.transfer.request",
        "provider.presets.describe.request",
        "provider.presets.list.request",
        "provider.presets.save.request",
        "provider.presets.delete.request",
    ] {
        assert_eq!(
            declarations.get(method),
            Some(&InboundKind::Request),
            "{method} declaration"
        );
        assert_eq!(
            installation.get(method),
            Some(&InboundKind::Request),
            "{method} installation"
        );
    }
}
