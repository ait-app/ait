use std::collections::BTreeMap;

use serde_json::json;

use super::{Preset, PresetBackend, PresetDescriptor, editable};

fn preset(name: &str, model: &str) -> Preset {
    Preset {
        name: name.to_owned(),
        model: model.to_owned(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    }
}

#[test]
fn an_ait_store_offers_rename_because_one_write_replaces_the_record() {
    let managed = PresetDescriptor::managed(PresetBackend::Ait);
    assert!(managed.can_create && managed.can_edit && managed.can_delete);
    // The Ait store locates the record by its original name and replaces it in one write, so
    // the editor may offer rename instead of asking a user to create-then-delete.
    assert!(managed.can_rename);
    assert_eq!(managed.reason, None);
}

#[test]
fn a_native_store_withholds_rename_because_it_has_no_such_command() {
    let native = PresetDescriptor::managed(PresetBackend::Native);
    assert!(native.can_create && native.can_edit && native.can_delete);
    assert!(
        !native.can_rename,
        "a native store can only save and delete, so a rename would not be atomic"
    );
}

#[test]
fn an_unavailable_store_withholds_every_operation_and_carries_the_reason() {
    let missing = PresetDescriptor::unavailable(PresetBackend::Native, "old OMP");
    assert!(!missing.can_create && !missing.can_edit && !missing.can_delete);
    assert!(!missing.can_rename);
    assert_eq!(missing.reason.as_deref(), Some("old OMP"));
}

#[test]
fn editable_presets_keep_unassigned_roles_and_remove_legacy_internal_markers() {
    let mut stored = preset("team", "model");
    stored.feature_values = BTreeMap::from([
        ("modelPreset".to_owned(), json!(null)),
        ("modelPresetThinking".to_owned(), json!("high")),
        ("modelPresetRoles".to_owned(), json!("{}")),
        ("modelRole.task".to_owned(), json!("unset")),
        ("modelRole.advisor".to_owned(), json!("native")),
    ]);
    assert_eq!(
        editable(stored).feature_values,
        BTreeMap::from([
            ("modelRole.task".to_owned(), json!("unset")),
            ("modelRole.advisor".to_owned(), json!("native")),
        ])
    );
}
