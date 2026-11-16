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

#[test]
fn owned_catalog_retains_variants_descriptions_and_visible_primary_agents() {
    let providers = json!({"providers":[{"id":"local","models":{
        "one":{"name":"First","variants":{"high":{},"off":{"disabled":true}}},
        "two":{"name":"Second"}
    }}]});
    let agents = vec![
        json!({"name":"build","mode":"primary","description":"Build description"}),
        json!({"name":"custom","mode":"all"}),
        json!({"name":"summary","mode":"primary","hidden":true}),
        json!({"name":"explore","mode":"subagent"}),
    ];

    let details = native(&providers, &agents).unwrap();

    let expected = parse_models("local/one\n{\"name\":\"First\",\"variants\":{\"high\":{},\"off\":{\"disabled\":true}}}\nlocal/two\n{\"name\":\"Second\"}\n").unwrap();
    assert_eq!(details.models, expected);
    assert_eq!(details.modes.len(), 2);
    assert_eq!(details.modes[0]["description"], "Build description");
    assert_eq!(details.modes[1]["id"], "custom");
}

#[test]
fn malformed_owned_catalogs_fail_before_publishing_partial_choices() {
    let agents = vec![json!({"name":"build","mode":"primary"})];
    for providers in [
        json!({}),
        json!({"providers":[{}]}),
        json!({"providers":[{"id":"local","models":[]}]}),
        json!({"providers":[{"id":"local","models":{"missing-name":{}}}]}),
        json!({"providers":[{"id":"local","models":{"bad\nid":{"name":"Bad"}}}]}),
    ] {
        assert!(native(&providers, &agents).is_err());
    }
    let providers = json!({"providers":[]});
    for agents in [
        vec![],
        vec![json!({"name":"build","mode":"invalid"})],
        vec![json!({"name":"--tool=shell","mode":"primary"})],
        vec![json!({"name":"build","mode":"primary"}); 129],
    ] {
        assert!(native(&providers, &agents).is_err());
    }
    assert!(model_entry("local/model", &json!({"name":"Model", "variants":
        (0..129).map(|index| (index.to_string(), json!({}))).collect::<serde_json::Map<_, _>>()
    })).is_err());
}
