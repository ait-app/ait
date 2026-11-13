use super::*;
use serde_json::json;

fn preset() -> ProviderPreset {
    ProviderPreset {
        name: "Team".to_owned(),
        model: "model".to_owned(),
        thinking_option_id: None,
        feature_values: BTreeMap::new(),
    }
}

#[test]
fn native_routes_and_dynamic_role_ids_fit_the_existing_adapter_bounds() {
    let mut value = preset();
    value.model = json!(["p".repeat(1024), "m".repeat(1024)]).to_string();
    value.feature_values = (0..128)
        .map(|index| {
            (
                format!("modelRole.{index:03}{}", "r".repeat(125)),
                json!("unset"),
            )
        })
        .collect();

    assert!(value.validate().is_ok());

    value
        .feature_values
        .insert("modelRole.extra".to_owned(), json!("native"));
    assert!(value.validate().is_err());
}

#[test]
fn invalid_roles_null_features_and_excessive_data_are_rejected() {
    for key in ["modelRole.", "modelRole.task\n", "1feature"] {
        let mut value = preset();
        value.feature_values.insert(key.to_owned(), json!("native"));
        assert!(value.validate().is_err(), "{key:?}");
    }
    for assignment in [Value::Null, json!("x".repeat(MAX_PRESET_BYTES))] {
        let mut value = preset();
        value
            .feature_values
            .insert("modelRole.task".to_owned(), assignment);
        assert!(value.validate().is_err());
    }
}

#[test]
fn explicit_unassignment_and_inheritance_remain_distinct_values() {
    let mut value = preset();
    value.feature_values = BTreeMap::from([
        ("modelRole.task".to_owned(), json!("unset")),
        ("modelRole.advisor".to_owned(), json!("native")),
    ]);

    let reopened: ProviderPreset =
        serde_json::from_value(serde_json::to_value(&value).unwrap()).unwrap();

    assert!(reopened.validate().is_ok());
    assert_eq!(reopened, value);
}

#[test]
fn stored_lists_reject_duplicate_names_and_too_many_presets() {
    assert!(validate_presets(&[]).is_ok());
    assert!(validate_presets(&[preset(), preset()]).is_err());
    let values: Vec<_> = (0..=MAX_PRESETS)
        .map(|index| ProviderPreset {
            name: format!("Team{index}"),
            ..preset()
        })
        .collect();
    assert!(validate_presets(&values).is_err());
}
