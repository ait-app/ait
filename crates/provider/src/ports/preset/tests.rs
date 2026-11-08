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
    // The thinking level is stated even when the preset has none: an omitted key means "keep the
    // current level" to the bundle, so only an explicit null clears it.
    assert_eq!(
        options[1]["metadata"]["configuration"],
        json!({"modelId":"claude-opus-5","thinkingOptionId":null})
    );
}

#[test]
fn a_preset_without_a_thinking_level_clears_it_instead_of_omitting_the_key() {
    let stored = vec![preset("team", "claude-opus-5")];
    let feature = synthesize(None, &stored, Some("sonnet"), None, None).unwrap();
    let configuration = &feature["options"][1]["metadata"]["configuration"];
    assert!(
        configuration.get("thinkingOptionId").is_some(),
        "an omitted field would keep the previous level instead of clearing it"
    );
    assert_eq!(configuration["thinkingOptionId"], serde_json::Value::Null);
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
        assert_eq!(
            feature["value"], "__custom__",
            "{model:?} {thinking:?} {role}"
        );
    }
}

#[test]
fn a_role_the_preset_never_named_must_be_cleared_for_it_to_stay_selected() {
    // Applying this preset clears every role it does not name, so a current selection that
    // enables one of those roles is a different configuration, not this preset.
    let stored = vec![preset("team", "claude-opus-5")];
    let enabled = BTreeMap::from([("modelRole.advisor".to_owned(), json!("opus"))]);
    let synthesized =
        synthesize(None, &stored, Some("claude-opus-5"), None, Some(&enabled)).unwrap();
    assert_eq!(synthesized["value"], "__custom__");

    // The same selection with that role cleared still matches the preset it applied.
    let cleared = BTreeMap::from([("modelRole.advisor".to_owned(), json!("unset"))]);
    let synthesized =
        synthesize(None, &stored, Some("claude-opus-5"), None, Some(&cleared)).unwrap();
    assert_eq!(synthesized["value"], "team");
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
