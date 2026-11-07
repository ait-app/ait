use std::collections::BTreeMap;

use serde_json::json;

use super::{Preset, PresetBackend, PresetDescriptor, synthesize};

fn preset(name: &str, model: &str) -> Preset {
    Preset {
        name: name.to_owned(),
        model: model.to_owned(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    }
}

#[test]
fn a_native_preset_feature_is_returned_untouched_and_never_augmented() {
    let native = json!({"type":"select","id":"modelPreset","value":"gpt",
        "options":[{"id":"__custom__","label":"Custom configuration"},{"id":"gpt","label":"gpt"}]});
    let stored = vec![preset("ait-only", "claude-opus-5")];
    let merged = synthesize(Some(native.clone()), &stored, Some("sonnet"), None, None);
    assert_eq!(merged, Some(native));
}

#[test]
fn stored_presets_are_synthesized_when_the_provider_has_no_native_feature() {
    let stored = vec![preset("team", "claude-opus-5")];
    let feature = synthesize(None, &stored, Some("sonnet"), None, None).unwrap();
    assert_eq!(feature["type"], "select");
    assert_eq!(feature["id"], "modelPreset");
    assert_eq!(feature["value"], "__custom__");
    let options = feature["options"].as_array().unwrap();
    assert_eq!(options.len(), 2);
    assert_eq!(options[0]["id"], "__custom__");
    assert_eq!(options[1]["id"], "team");
    assert_eq!(
        options[1]["metadata"]["configuration"],
        json!({"modelId":"claude-opus-5"})
    );
}

#[test]
fn neither_store_produces_no_feature_at_all() {
    assert_eq!(synthesize(None, &[], Some("sonnet"), None, None), None);
}

#[test]
fn the_current_selection_selects_matching_preset_and_otherwise_custom() {
    let mut team = preset("team", "claude-opus-5");
    team.thinking_option_id = Some("high".to_owned());
    team.feature_values
        .insert("modelRole.advisor".to_owned(), json!("sonnet"));
    let stored = vec![team];
    let features = BTreeMap::from([("modelRole.advisor".to_owned(), json!("sonnet"))]);

    let matched = synthesize(
        None,
        &stored,
        Some("claude-opus-5"),
        Some("high"),
        Some(&features),
    )
    .unwrap();
    assert_eq!(matched["value"], "team");
    assert_eq!(
        matched["options"][1]["metadata"]["configuration"]["featureValues"]["modelRole.advisor"],
        "sonnet"
    );

    // A different model, a different effort, and an overridden role all fall back to custom.
    for (model, thinking, role) in [
        (Some("claude-sonnet-5"), Some("high"), "sonnet"),
        (Some("claude-opus-5"), None, "sonnet"),
        (Some("claude-opus-5"), Some("high"), "opus"),
    ] {
        let features = BTreeMap::from([("modelRole.advisor".to_owned(), json!(role))]);
        let feature = synthesize(None, &stored, model, thinking, Some(&features)).unwrap();
        assert_eq!(feature["value"], "__custom__", "{model:?} {thinking:?} {role}");
    }
}

#[test]
fn descriptors_report_a_native_store_as_unable_to_rename() {
    let managed = PresetDescriptor::managed(PresetBackend::Ait);
    assert!(managed.can_create && managed.can_edit && managed.can_delete && managed.can_rename);
    assert_eq!(managed.reason, None);

    let missing = PresetDescriptor::unavailable(PresetBackend::Native, "old OMP");
    assert!(!missing.can_create && !missing.can_edit && !missing.can_delete);
    assert!(!missing.can_rename);
    assert_eq!(missing.reason.as_deref(), Some("old OMP"));
}
