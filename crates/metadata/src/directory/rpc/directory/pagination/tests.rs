//! Adapted from Paseo pagination/{cursor,sortable-pager}.test.ts.

use super::*;

#[test]
fn normalizes_default_and_duplicate_sort_keys() {
    let default = normalize_sort(None);
    assert_eq!(default, normalize_sort(Some(&[])));
    assert_eq!(default[0].key, WorkspaceSortKey::ActivityAt);
    assert_eq!(default[0].direction, SortDirection::Desc);
    let first = WorkspaceSort {
        key: WorkspaceSortKey::Name,
        direction: SortDirection::Asc,
    };
    let duplicate = WorkspaceSort {
        direction: SortDirection::Desc,
        ..first
    };
    assert_eq!(
        normalize_sort(Some(&[first, duplicate, default[0]])),
        [first, default[0]]
    );
}
