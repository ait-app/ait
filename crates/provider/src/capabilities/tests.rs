use std::collections::BTreeSet;

use super::*;

#[test]
fn installation_selects_the_entire_crate() {
    let declared: Vec<_> = implemented_methods().collect();
    let names: BTreeSet<_> = declared.iter().map(|spec| spec.name).collect();
    assert_eq!(declared.len(), 46);
    assert_eq!(declared.len(), names.len());
    assert_eq!(installed_methods(true).collect::<Vec<_>>(), declared);
    assert_eq!(installed_methods(false).count(), 0);
}
