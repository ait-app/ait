use serde_json::json;

use super::*;

#[test]
fn full_configuration_rejects_empty_identifiers_and_zero_limits() {
    for (field, value) in [
        (
            "git",
            json!({"maxProcessesPerSecond":0,"maxProcessConcurrency":1}),
        ),
        (
            "git",
            json!({"maxProcessesPerSecond":1,"maxProcessConcurrency":0}),
        ),
        ("catalogRefreshTimeoutMs", json!(0)),
        ("providers", json!({"":{}})),
        (
            "providers",
            json!({"codex":{"additionalModels":[{"id":"","label":"Model"}]}}),
        ),
        (
            "providers",
            json!({"codex":{"additionalModels":[{"id":"custom","label":""}]}}),
        ),
        ("metadataGeneration", json!({"providers":[{"provider":""}]})),
        (
            "metadataGeneration",
            json!({"providers":[{"provider":"codex","model":""}]}),
        ),
        (
            "metadataGeneration",
            json!({"providers":[{"provider":"codex","thinkingOptionId":""}]}),
        ),
    ] {
        let mut config = serde_json::to_value(DaemonConfig::default()).unwrap();
        config[field] = value;
        let parsed: DaemonConfig = serde_json::from_value(config.clone()).unwrap();
        assert!(parsed.validate().is_err(), "{config}");
    }
    let mut config = serde_json::to_value(DaemonConfig::default()).unwrap();
    config["providers"] = json!({"codex":{"additionalModels":[{"id":"custom","label":"Model"}]}});
    config["metadataGeneration"] =
        json!({"providers":[{"provider":"codex","model":"custom","thinkingOptionId":"high"}]});
    serde_json::from_value::<DaemonConfig>(config)
        .unwrap()
        .validate()
        .unwrap();
}

#[test]
fn partial_configuration_validates_nested_values_without_requiring_a_full_document() {
    for invalid in [
        json!({"relay":[]}),
        json!({"mcp":false}),
        json!({"browserTools":"yes"}),
        json!({"relay":{"enabled":1}}),
        json!({"mcp":{"injectIntoAgents":"yes"}}),
        json!({"browserTools":{"enabled":[]}}),
        json!({"metadataGeneration":{"providers":{}}}),
        json!({"metadataGeneration":{"providers":[false]}}),
        json!({"metadataGeneration":{"providers":[{"provider":""}]}}),
        json!({"providers":{"codex":[]}}),
        json!({"providers":{"codex":{"enabled":1}}}),
        json!({"providers":{"codex":{"additionalModels":{}}}}),
        json!({"providers":{"codex":{"additionalModels":[false]}}}),
        json!({"providers":{"codex":{"additionalModels":[{"id":"custom"}]}}}),
        json!({"providers":{"codex":{"additionalModels":[{"id":"","label":"Model"}]}}}),
        json!({"removeProviders":[""]}),
    ] {
        let patch: DaemonConfigPatch = serde_json::from_value(invalid.clone()).unwrap();
        assert!(patch.validate().is_err(), "{invalid}");
    }
    for valid in [
        json!({}),
        json!({"relay":{},"mcp":{},"browserTools":{}}),
        json!({"metadataGeneration":{"providers":[{"provider":"codex"}]}}),
        json!({"providers":{"codex":{"additionalModels":[{"id":"custom","label":"Model","future":true}]}}}),
        json!({"removeProviders":["codex"]}),
    ] {
        serde_json::from_value::<DaemonConfigPatch>(valid.clone())
            .unwrap()
            .validate()
            .unwrap();
    }
}

#[test]
fn status_and_pairing_shapes_match_paseo_payload_fields() {
    let status = DaemonStatus {
        server_id: "srv-1".to_owned(),
        version: Some("1.2.3".to_owned()),
        pid: 42,
        node_path: "/bin/server".to_owned(),
        started_at: None,
        listen: Some("127.0.0.1:6767".to_owned()),
        relay: None,
        providers: vec![ProviderAvailability {
            provider: "codex".to_owned(),
            available: false,
            error: Some("missing".to_owned()),
        }],
    };
    assert_eq!(
        serde_json::to_value(status).unwrap(),
        json!({
            "serverId":"srv-1", "version":"1.2.3", "pid":42,
            "nodePath":"/bin/server", "startedAt":null,
            "listen":"127.0.0.1:6767", "relay":null,
            "providers":[{"provider":"codex","available":false,"error":"missing"}]
        })
    );
}

#[test]
fn config_defaults_and_passthrough_match_paseo_mutable_shape() {
    let config = DaemonConfig::default();
    config.validate().unwrap();
    let value = serde_json::to_value(&config).unwrap();
    assert_eq!(value["relay"]["enabled"], false);
    assert_eq!(value["mcp"]["enabled"], true);
    assert_eq!(value["mcp"]["injectIntoAgents"], false);
    assert_eq!(value["browserTools"]["enabled"], false);
    assert_eq!(value["metadataGeneration"]["providers"], json!([]));

    let parsed: DaemonConfig = serde_json::from_value(json!({
        "relay":{"enabled":false,"future":1},
        "mcp":{"injectIntoAgents":false},
        "browserTools":{"enabled":false},
        "providers":{},
        "metadataGeneration":{"providers":[]},
        "autoArchiveAfterMerge":false,
        "enableTerminalAgentHooks":false,
        "appendSystemPrompt":"",
        "futureRoot":true
    }))
    .unwrap();
    assert_eq!(parsed.relay.unwrap().extra["future"], 1);
    assert_eq!(parsed.extra["futureRoot"], true);
}

#[test]
fn patch_accepts_unknown_fields_but_rejects_invalid_known_shapes() {
    let patch: DaemonConfigPatch = serde_json::from_value(json!({
        "relay":{"enabled":true},
        "providers":{"codex":{"enabled":false}},
        "futureRoot":{"value":1}
    }))
    .unwrap();
    patch.validate().unwrap();
    assert!(patch.extra.contains_key("futureRoot"));

    let invalid: DaemonConfigPatch = serde_json::from_value(json!({"providers":{"":{}}})).unwrap();
    assert!(invalid.validate().is_err());
    let invalid: DaemonConfigPatch =
        serde_json::from_value(json!({"metadataGeneration":[]})).unwrap();
    assert!(invalid.validate().is_err());
}

#[test]
fn update_reload_and_lifecycle_results_use_canonical_payloads() {
    assert_eq!(
        serde_json::to_value(ConfigReloadResult {
            applied_paths: vec!["daemon.browserTools.enabled".to_owned()],
            restart_required_paths: Vec::new(),
            override_controlled_paths: Vec::new(),
        })
        .unwrap()["appliedPaths"],
        json!(["daemon.browserTools.enabled"])
    );
    assert_eq!(
        serde_json::to_value(DaemonUpdateResult {
            success: false,
            error: Some("unsupported".to_owned()),
            previous_version: Some("0.0.6".to_owned()),
            new_version: None,
        })
        .unwrap(),
        json!({
            "success":false, "error":"unsupported",
            "previousVersion":"0.0.6", "newVersion":null
        })
    );
    assert_eq!(
        serde_json::to_value(LifecycleResult {
            status: "restart_requested".to_owned(),
            reason: Some("settings_changed".to_owned()),
        })
        .unwrap(),
        json!({"status":"restart_requested","reason":"settings_changed"})
    );
}
