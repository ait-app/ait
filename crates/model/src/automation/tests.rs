use super::*;

fn settings() -> Settings {
    Settings {
        enabled: true,
        roots: vec![Root {
            path: "/work".to_owned(),
            label: Some("work".to_owned()),
        }],
        providers: vec!["codex".to_owned()],
    }
}

#[test]
fn defaults_are_off_and_valid() {
    let settings = Settings::default();
    assert!(!settings.enabled);
    assert!(settings.is_valid());
    assert!(settings.admits_provider("anything"));
}

#[test]
fn rejects_relative_roots_bad_labels_and_providers() {
    assert!(settings().is_valid());
    let mut relative = settings();
    relative.roots[0].path = "work".to_owned();
    assert!(!relative.is_valid());
    let mut empty_label = settings();
    empty_label.roots[0].label = Some(String::new());
    assert!(!empty_label.is_valid());
    let mut provider = settings();
    provider.providers = vec!["Codex".to_owned()];
    assert!(!provider.is_valid());
    let mut many = settings();
    many.providers = vec!["p".to_owned(); MAX_PROVIDERS + 1];
    assert!(!many.is_valid());
}

#[test]
fn limits_providers_and_publishes_version_one() {
    let settings = settings();
    assert!(settings.admits_provider("codex"));
    assert!(!settings.admits_provider("claude"));
    assert_eq!(
        settings.publication(),
        serde_json::json!({"version":1,"enabled":true,
            "roots":[{"path":"/work","label":"work"}],"providers":["codex"]})
    );
}
