use super::*;
use serde_json::json;

#[test]
fn values_are_preserved_for_launch_and_redacted_from_debug() {
    let environment: AgentEnvironment =
        serde_json::from_value(json!({"TOKEN":"private-value","EMPTY":"","UNICODE":"中文"}))
            .unwrap();
    assert!(!environment.is_empty());
    let exposed: BTreeMap<_, _> = environment.entries().collect();
    assert_eq!(exposed["TOKEN"], "private-value");
    assert_eq!(exposed["EMPTY"], "");
    assert_eq!(exposed["UNICODE"], "中文");
    assert!(!format!("{environment:?}").contains("private-value"));
    assert!(AgentEnvironment::default().is_empty());
}

#[test]
fn malformed_and_unbounded_environment_values_are_rejected_before_launch() {
    for value in [
        json!(null),
        json!([]),
        json!({"":"value"}),
        json!({"A=B":"value"}),
        json!({"A\u{0}B":"value"}),
        json!({"A":"with\u{0}nul"}),
        json!({"A":false}),
        json!({"A":"x".repeat(65_537)}),
        json!({"x".repeat(257):"value"}),
        json!(
            (0..257)
                .map(|index| (format!("KEY{index}"), "value"))
                .collect::<BTreeMap<_, _>>()
        ),
        json!(
            (0..5)
                .map(|index| (format!("KEY{index}"), "x".repeat(65_536)))
                .collect::<BTreeMap<_, _>>()
        ),
    ] {
        assert!(serde_json::from_value::<AgentEnvironment>(value).is_err());
    }
}

#[test]
fn variable_prefers_overrides_and_falls_back_to_the_daemon_environment() {
    let environment: AgentEnvironment =
        serde_json::from_value(json!({"HOME":"/override/home"})).unwrap();

    assert_eq!(
        environment.variable("HOME").as_deref(),
        Some("/override/home")
    );
    assert_eq!(
        environment.variable("PATH"),
        std::env::var("PATH").ok(),
        "unset overrides read the daemon environment"
    );
    assert_eq!(
        environment.variable("AIT_SANDBOX_TEST_UNSET_VARIABLE_7F3A"),
        None
    );
}
