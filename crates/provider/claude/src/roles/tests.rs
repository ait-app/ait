use std::collections::BTreeMap;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};

use super::{INHERIT, UNSET, check_size, environment, features, is_key, valid};
use crate::config;

fn with_roles(entries: &[(&str, &str)]) -> StoredAgentConfig {
    StoredAgentConfig {
        feature_values: Some(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_owned(), json!(value)))
                .collect::<BTreeMap<String, Value>>(),
        ),
        ..StoredAgentConfig::default()
    }
}

#[test]
fn every_known_lever_is_a_role_key() {
    for key in [
        "modelRole.subagent",
        "modelRole.workflow",
        "modelRole.small-fast",
        "modelRole.opus",
        "modelRole.sonnet",
        "modelRole.haiku",
        "modelRole.fable",
    ] {
        assert!(is_key(key), "{key} should be a role key");
        assert!(
            valid(key, &json!("some-model")),
            "{key} should accept a model"
        );
    }
    // Effort is the one enumerated lever, so it is a role key that rejects a model id.
    assert!(is_key("modelRole.subagent-effort"));
    assert!(!valid("modelRole.subagent-effort", &json!("some-model")));
}

#[test]
fn unknown_roles_and_fast_mode_are_not_role_keys() {
    for key in [
        "modelRole.ghost",
        "fast_mode",
        "modelRole.",
        "modelRole.task",
    ] {
        assert!(!is_key(key), "{key} should not be a role key");
        assert!(!valid(key, &json!("x")), "{key} should not validate");
    }
}

#[test]
fn effort_levels_are_enumerated_but_models_are_open() {
    assert!(valid("modelRole.subagent-effort", &json!("xhigh")));
    assert!(!valid("modelRole.subagent-effort", &json!("ultracode")));
    assert!(valid("modelRole.subagent", &json!("claude-opus-5-5")));
}

#[test]
fn non_string_and_control_values_are_rejected() {
    assert!(!valid("modelRole.subagent", &json!(7)));
    assert!(!valid("modelRole.subagent", &json!("bad\nvalue")));
    assert!(!valid("modelRole.subagent", &json!("")));
}

#[test]
fn inherit_emits_nothing_and_unset_emits_an_empty_value() {
    let config = with_roles(&[
        ("modelRole.opus", INHERIT),
        ("modelRole.haiku", UNSET),
        ("modelRole.subagent", "claude-opus-5-5"),
    ]);
    let mut actual = environment(&config);
    actual.sort_unstable();
    assert_eq!(
        actual,
        vec![
            ("ANTHROPIC_DEFAULT_HAIKU_MODEL", String::new()),
            ("CLAUDE_CODE_SUBAGENT_MODEL", "claude-opus-5-5".to_owned()),
        ]
    );
}

#[test]
fn unknown_roles_never_reach_the_environment() {
    let config = with_roles(&[("modelRole.ghost", "model")]);
    assert!(environment(&config).is_empty());
}

#[test]
fn stored_roles_render_as_selects_with_inherit_and_unset() {
    let config = with_roles(&[("modelRole.opus", "renamed-model")]);
    let models = vec![json!({"id":"opus","label":"Opus 5"})];
    let rendered = features(&config, &models);
    let opus = rendered
        .iter()
        .find(|feature| feature["id"] == "modelRole.opus")
        .expect("opus role is rendered");
    assert_eq!(opus["value"], "renamed-model");
    assert_eq!(opus["type"], "select");
    let ids: Vec<&str> = opus["options"]
        .as_array()
        .expect("options")
        .iter()
        .filter_map(|option| option["id"].as_str())
        .collect();
    // The unknown stored value stays selectable instead of silently rerouting the role.
    assert!(ids.contains(&INHERIT));
    assert!(ids.contains(&UNSET));
    assert!(ids.contains(&"opus"));
    assert!(ids.contains(&"renamed-model"));
}

#[test]
fn a_role_validates_through_the_provider_config_gate() {
    assert!(config::validate(&with_roles(&[("modelRole.subagent", "claude-opus-5-5")])).is_ok());
    assert!(config::validate(&with_roles(&[("modelRole.ghost", "model")])).is_err());
    assert!(config::validate(&with_roles(&[("modelRole.subagent-effort", "ultracode")])).is_err());
    // Fast mode stays a boolean toggle and is not mistaken for a role.
    assert!(config::validate(&with_roles(&[("fast_mode", "true")])).is_err());
}

#[test]
fn oversized_role_sets_are_rejected() {
    assert!(check_size(&StoredAgentConfig::default()).is_ok());
    let huge = "m".repeat(2_048);
    assert!(check_size(&with_roles(&[("modelRole.subagent", &huge)])).is_err());
}
