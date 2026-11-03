use super::*;

fn catalog() -> Catalog {
    Catalog::parse(&json!({"key":"modelRoles","value":{"default":"chat/text","smol":"@default","advisor":"chat/vision:high"}}),
        &json!({"key":"modelTags","value":{"custom":{"name":"My role"}}}),
        &json!({"key":"cycleOrder","value":[]}),
        &json!({"models":[
            {"selector":"chat/text","name":"Text","kind":"chat","input":["text"]},
            {"selector":"chat/vision","name":"Vision","kind":"chat","input":["text","image"]},
            {"selector":"local/tiny","name":"Tiny","kind":"tiny"},
            {"selector":"local/image","name":"Image","kind":"image"},
            {"selector":"local/search","name":"Search","kind":"search"},
            {"selector":"local/speech","name":"Speech","kind":"tts"},
            {"selector":"local/listen","name":"Listen","kind":"stt"},
            {"selector":"local/judge","name":"Judge","kind":"judge"}
        ]})).unwrap()
}

fn config(role: &str, selector: &str) -> StoredAgentConfig {
    StoredAgentConfig {
        feature_values: Some(BTreeMap::from([(
            format!("{PREFIX}{role}"),
            json!(selector),
        )])),
        ..StoredAgentConfig::default()
    }
}

#[test]
fn role_choices_follow_real_native_catalog_kinds_and_preserve_inherited_selectors() {
    let catalog = catalog();
    let features = catalog.features(&StoredAgentConfig::default());
    assert_eq!(features.len(), 17);
    for (role, count) in [
        ("default", 2),
        ("smol", 2),
        ("vision", 2),
        ("advisor", 2),
        ("image", 1),
        ("tiny", 3),
        ("memory", 3),
        ("web", 1),
        ("speech", 1),
        ("dictation", 1),
        ("judge", 4),
        ("custom", 2),
    ] {
        let feature = features
            .iter()
            .find(|feature| feature["id"] == format!("{PREFIX}{role}"))
            .unwrap();
        assert_eq!(feature["value"], INHERIT);
        assert_eq!(
            feature["options"].as_array().unwrap().len(),
            count + 1,
            "{role}"
        );
    }
    let smol = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.smol")
        .unwrap();
    assert_eq!(smol["options"][0]["label"], "Use OMP setting (@default)");
    let custom = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.custom")
        .unwrap();
    assert_eq!(custom["label"], "My role model (custom)");
}

#[test]
fn role_assignments_reject_missing_models_unknown_roles_and_incompatible_kinds() {
    let catalog = catalog();
    for (role, selector) in [
        ("default", "chat/text"),
        ("vision", "chat/vision"),
        ("advisor", "chat/text"),
        ("image", "local/image"),
        ("custom", "chat/vision"),
        ("smol", INHERIT),
    ] {
        let config = config(role, selector);
        assert!(catalog.validate(&config).is_ok());
        assert!(valid(&format!("{PREFIX}{role}"), &json!(selector)));
    }
    for (role, selector) in [
        ("missing", "chat/text"),
        ("image", "chat/vision"),
        ("advisor", "local/speech"),
        ("smol", "unknown/model"),
    ] {
        assert!(
            catalog.validate(&config(role, selector)).is_err(),
            "{role}={selector}"
        );
    }
    assert!(!valid(PREFIX, &json!(INHERIT)));
    assert!(!valid("modelRole.smol", &json!(true)));
    assert!(!valid("modelRole.smol", &json!("command\nvalue")));
}

#[test]
fn explicit_roles_offer_and_validate_only_native_thinking_suffixes() {
    let mut catalog = catalog();
    catalog.models[0].thinking = Some(vec!["low".to_owned(), "high".to_owned()]);
    assert!(
        catalog
            .validate(&config("advisor", "chat/text:high"))
            .is_ok()
    );
    assert!(
        catalog
            .validate(&config("advisor", "chat/text:unsupported"))
            .is_err()
    );
    assert!(
        catalog
            .validate(&config("advisor", "chat/vision:high"))
            .is_err()
    );
    let features = catalog.features(&config("advisor", "chat/text:low"));
    let advisor = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.advisor")
        .unwrap();
    assert_eq!(advisor["value"], "chat/text:low");
    assert!(
        advisor["options"]
            .as_array()
            .unwrap()
            .iter()
            .any(|option| option["id"] == "chat/text:high")
    );
}

#[tokio::test]
async fn private_overlays_contain_only_explicit_roles_and_stored_controls_survive_without_discovery()
 {
    assert!(
        overlay(&StoredAgentConfig::default())
            .await
            .unwrap()
            .is_none()
    );
    let mut config = config("advisor", "chat/vision");
    config
        .feature_values
        .as_mut()
        .unwrap()
        .insert("modelRole.smol".into(), json!(INHERIT));
    let overlay = overlay(&config).await.unwrap().unwrap();
    let actual: Value = serde_json::from_slice(&std::fs::read(overlay.path()).unwrap()).unwrap();
    assert_eq!(actual, json!({"modelRoles":{"advisor":"chat/vision"}}));
    let features = stored_features(&config);
    let advisor = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.advisor")
        .unwrap();
    assert_eq!(advisor["value"], "chat/vision");
    assert_eq!(advisor["options"][1]["id"], "chat/vision");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(overlay.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
}

#[test]
fn malformed_native_role_catalogs_fail_without_publishing_guessed_choices() {
    for roles in [
        json!({}),
        json!({"key":"other","value":{}}),
        json!({"key":"modelRoles","value":{"smol":null}}),
    ] {
        assert!(
            Catalog::parse(
                &roles,
                &json!({"key":"modelTags","value":{}}),
                &json!({"key":"cycleOrder","value":[]}),
                &json!({"models":[]})
            )
            .is_err()
        );
    }
    assert!(
        Catalog::parse(
            &json!({"key":"modelRoles","value":{}}),
            &json!({"key":"modelTags","value":{}}),
            &json!({"key":"cycleOrder","value":[]}),
            &json!({"models":[{"selector":"not-canonical","name":"Bad","kind":"chat"}]})
        )
        .is_err()
    );
}
