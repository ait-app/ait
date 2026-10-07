use super::*;
use serde_json::json;

fn selection(provider: &str, model: &str) -> MetadataSelection {
    MetadataSelection {
        provider: provider.into(),
        model: Some(model.into()),
        thinking_option_id: None,
    }
}

#[test]
fn overrides_precede_provider_defaults_without_foreground_model_fallback() {
    let automatic = BTreeMap::from([
        ("alpha".into(), selection("alpha", "small-a")),
        ("beta".into(), selection("beta", "small-b")),
    ]);
    let config = json!({"metadataGeneration":{"providers":[
        {"provider":" alpha ","model":"custom"}, {"provider":"beta"}, {"provider":"unknown"}
    ]}});
    let result = ordered(
        ["alpha", "beta"].into_iter(),
        &config,
        Some(&selection("alpha", "large")),
        &automatic,
    );
    assert_eq!(
        result,
        vec![
            selection("alpha", "custom"),
            selection("beta", "small-b"),
            selection("alpha", "small-a")
        ]
    );
}

#[test]
fn unavailable_defaults_and_disabled_providers_never_become_candidates() {
    let automatic = BTreeMap::from([("alpha".into(), selection("alpha", "small"))]);
    let config = json!({"providers":{"alpha":{"enabled":false}},"metadataGeneration":{"providers":[{"provider":"alpha","model":"custom"},{"provider":"beta"}]}});
    assert!(
        ordered(
            ["alpha", "beta"].into_iter(),
            &config,
            Some(&selection("beta", "large")),
            &automatic
        )
        .is_empty()
    );
    let explicit =
        json!({"metadataGeneration":{"providers":[{"provider":"beta","model":"custom"}]}});
    assert_eq!(
        ordered(["beta"].into_iter(), &explicit, None, &BTreeMap::new()),
        vec![selection("beta", "custom")]
    );
}
