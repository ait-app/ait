use super::*;

#[test]
fn native_catalogs_keep_model_efforts_and_primary_modes_without_guessing_defaults() {
    let models = parse_models(concat!(
        "local/one\n{\"name\":\"First\",\"variants\":{\"high\":{},",
        "\"off\":{\"disabled\":true}}}\nlocal/two\n{\"name\":\"Second\"}\n"
    ))
    .unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0]["thinkingOptions"],
        json!([{"id":"high","label":"high"}])
    );
    assert_eq!(models[0]["isDefault"], false);
    let available_agents =
        parse_modes("build (primary)\n  []\ncustom (all)\n  []\nexplore (subagent)\n  []").unwrap();
    assert_eq!(available_agents.len(), 2);
    assert_eq!(available_agents[1]["id"], "custom");
}

#[test]
fn malformed_catalogs_fail_instead_of_returning_partial_choices() {
    for output in [
        "local/one",
        "one\n{}",
        "local/one\n{}",
        "local/one\nnot json",
        "local/one\n{\"name\":\"First\"}\ntruncated",
    ] {
        assert!(parse_models(output).is_err(), "{output}");
    }
    assert!(parse_modes("explore (subagent)\n  []").is_err());
    assert!(parse_modes(" (primary)").is_err());
    assert!(parse_modes("--tool=shell (primary)").is_err());
}
