use super::*;

fn config(value: Value) -> StoredAgentConfig {
    serde_json::from_value(value).unwrap()
}

#[test]
fn executable_discovery_accepts_path_search_and_rejects_directories_or_missing_programs() {
    assert!(executable(std::path::Path::new("cargo")));
    assert!(!executable(std::path::Path::new(
        "ait-fixture-program-that-does-not-exist"
    )));
    let root = tempfile::tempdir().unwrap();
    assert!(!executable(root.path()));
    assert!(!executable(&root.path().join("missing")));
}

#[test]
fn codex_translates_transports_and_limits_preapprovals_to_named_tools() {
    let config = config(json!({"thinkingOptionId":"high", "providerOptions":{
        "sandbox_mode":"workspace-write", "web_search":"live",
        "features":{"network_proxy":{"domains":{"example.com":"allow"}}}},
        "mcpServers":{
            "local":{"type":"stdio","command":"node","args":["server.js"],"env":{"DEMO":"test"}},
            "remote":{"type":"http","url":"https://example.com/mcp","headers":{"X-Test":"fixture"}},
            "events":{"type":"sse","url":"http://localhost:3210/events"}},
        "toolPolicy":{"preapproved":[{"kind":"mcp","server":"local","tool":"read"},
            {"kind":"mcp","server":"local","tool":"read"},
            {"kind":"mcp","server":"inherited","tool":"search"}]}}));
    let native = codex(&config).unwrap();
    assert_eq!(native["mcp_servers"]["local"]["command"], "node");
    assert_eq!(native["mcp_servers"]["local"]["env"]["DEMO"], "test");
    assert_eq!(
        native["mcp_servers"]["local"]["enabled_tools"],
        json!(["read"])
    );
    assert_eq!(
        native["mcp_servers"]["inherited"]["tools"]["search"]["approval_mode"],
        "approve"
    );
    assert_eq!(
        native["mcp_servers"]["inherited"]["default_tools_approval_mode"],
        "prompt"
    );
    assert_eq!(
        native["mcp_servers"]["remote"]["http_headers"]["X-Test"],
        "fixture"
    );
    assert_eq!(
        native["mcp_servers"]["events"]["url"],
        "http://localhost:3210/events"
    );
    assert_eq!(native["model_reasoning_effort"], "high");
    assert_eq!(native["web_search"], "live");
    assert!(native["mcp_servers"]["local"].get("type").is_none());
}

#[test]
fn codex_auto_review_and_full_access_preapprove_only_the_bundled_cua_js_tool() {
    for mode in ["auto-review", "full-access"] {
        let config = config(json!({"modeId":mode}));
        let native = codex(&config).unwrap();
        assert!(codex_computer_use_auto_approval(&config));
        assert_eq!(
            native,
            json!({"plugins":{"unified-computer-use@openai-bundled":{
                "mcp_servers":{"cua_repl":{"tools":{"js":{"approval_mode":"approve"}}}}}}})
        );
    }
    for value in [
        json!({}),
        json!({"modeId":"auto"}),
        json!({"modeId":"read-only"}),
    ] {
        let config = config(value);
        assert!(!codex_computer_use_auto_approval(&config));
        assert!(codex(&config).unwrap().get("plugins").is_none());
    }
}

#[test]
fn codex_explicit_plugin_tool_approval_overrides_computer_use_presets() {
    for mode in ["auto", "prompt", "writes", "approve"] {
        let plugins = json!({"unified-computer-use@openai-bundled":{
            "mcp_servers":{"cua_repl":{"tools":{"js":{"approval_mode":mode}}}}},
            "other@local":{"mcp_servers":{"docs":{"tools":{"search":{"approval_mode":"prompt"}}}}}});
        for mode_id in ["auto-review", "full-access"] {
            let config = config(json!({"modeId":mode_id,
                "providerOptions":{"plugins":plugins}}));
            let native = codex(&config).unwrap();
            assert_eq!(codex_computer_use_auto_approval(&config), mode == "approve");
            assert_eq!(native["plugins"], plugins);
            assert!(native.get("mcp_servers").is_none());
        }
    }
}

#[test]
fn claude_merges_sandbox_settings_without_losing_permission_rules() {
    let config = config(
        json!({"featureValues":{"fast_mode":true},"providerOptions":{
        "allowedTools":["Read"],"disallowedTools":["Bash"],"additionalDirectories":["/tmp/a b"],
        "settings":{"permissions":{"ask":["Edit"]},"sandbox":{"network":{"allowedDomains":["example.com"]}}},
        "sandbox":{"enabled":true,"network":{"allowLocalBinding":true}}},
        "toolPolicy":{"preapproved":[{"kind":"mcp","server":"docs","tool":"search"}]},
        "mcpServers":{"docs":{"type":"stdio","command":"node","args":["literal$(value)"]}}}),
    );
    let args = claude(&config, false).unwrap();
    assert!(args.contains(&"--allowedTools=Read,mcp__docs__search".to_owned()));
    assert!(args.contains(&"--disallowedTools=Bash".to_owned()));
    assert!(args.contains(&"--add-dir=/tmp/a b".to_owned()));
    let settings: Value = serde_json::from_str(
        args.iter()
            .find_map(|arg| arg.strip_prefix("--settings="))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(settings["permissions"]["ask"], json!(["Edit"]));
    assert_eq!(
        settings["sandbox"]["network"]["allowedDomains"],
        json!(["example.com"])
    );
    assert_eq!(settings["sandbox"]["network"]["allowLocalBinding"], true);
    assert_eq!(settings["fastMode"], true);
    assert!(args.iter().all(|arg| !arg.contains("literal$(value)")));
    let mcp = claude_mcp(&config).unwrap();
    assert_eq!(
        mcp["mcpServers"]["docs"]["args"],
        json!(["literal$(value)"])
    );
    assert_eq!(claude_mcp(&StoredAgentConfig::default()), None);
}

#[test]
fn claude_emits_role_environment_through_the_settings_document() {
    let environment = [
        ("CLAUDE_CODE_SUBAGENT_MODEL", "claude-opus-5-5".to_owned()),
        // An unset lever travels as an explicit empty value, which is how Claude Code turns it
        // off; dropping the key instead would silently leave the user's own value in force.
        ("ANTHROPIC_DEFAULT_HAIKU_MODEL", String::new()),
    ];
    let args = claude_with_environment(&StoredAgentConfig::default(), false, &environment).unwrap();
    let settings: Value = serde_json::from_str(
        args.iter()
            .find_map(|arg| arg.strip_prefix("--settings="))
            .expect("role environment produces a settings argument"),
    )
    .unwrap();
    assert_eq!(
        settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
        json!("claude-opus-5-5")
    );
    assert_eq!(settings["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], json!(""));
}

#[test]
fn an_empty_role_environment_adds_no_settings_argument() {
    let args = claude_with_environment(&StoredAgentConfig::default(), false, &[]).unwrap();
    assert!(
        args.iter().all(|arg| !arg.starts_with("--settings=")),
        "no role assignment and no other option means no settings document"
    );
}

#[test]
fn rejects_unknown_invalid_or_overbroad_advanced_configuration() {
    for (provider, value) in [
        ("codex", json!({"providerOptions":{"unknown":true}})),
        ("codex", json!({"providerOptions":{"approval_policy":{}}})),
        (
            "codex",
            json!({"providerOptions":{"plugins":{"cua":{"enabled":true}}}}),
        ),
        (
            "codex",
            json!({"providerOptions":{"plugins":{"cua":{"mcp_servers":{"cua_repl":{
                "default_tools_approval_mode":"approve"}}}}}}),
        ),
        (
            "codex",
            json!({"providerOptions":{"plugins":{"cua":{"mcp_servers":{"cua_repl":{
                "tools":{"js":{"approval_mode":"invalid"}}}}}}}}),
        ),
        (
            "claude",
            json!({"providerOptions":{"plugins":{"cua":{"mcp_servers":{}}}}}),
        ),
        (
            "codex",
            json!({"providerOptions":{"features":{"network_proxy":{"domains":{"a":"maybe"}}}}}),
        ),
        (
            "claude",
            json!({"providerOptions":{"sandbox":{"network":{"httpProxyPort":65536}}}}),
        ),
        (
            "claude",
            json!({"providerOptions":{"sandbox":{"ripgrep":{}}}}),
        ),
        (
            "claude",
            json!({"providerOptions":{"settings":{"permissions":{"trust":true}}}}),
        ),
        (
            "claude",
            json!({"mcpServers":{"a":{"type":"stdio","command":"node","url":"https://a"}}}),
        ),
        (
            "codex",
            json!({"mcpServers":{"a":{"type":"http","url":"file:///etc/passwd"}}}),
        ),
        (
            "claude",
            json!({"toolPolicy":{"preapproved":[{"kind":"mcp","server":"a","tool":"*"}]}}),
        ),
        (
            "claude",
            json!({"toolPolicy":{"preapproved":[{"kind":"mcp","server":"a__b","tool":"c"}]}}),
        ),
        (
            "codex",
            json!({"toolPolicy":{"preapproved":[],"allowAll":true}}),
        ),
        ("codex", json!({"systemPrompt":"bad\u{0}prompt"})),
    ] {
        assert_eq!(
            validate(&config(value.clone()), provider),
            Err(AgentSessionError::Rejected),
            "{provider}: {value}"
        );
    }
    let oversized = config(json!({"providerOptions":{"allowedTools":["a".repeat(256*1024)]}}));
    assert_eq!(
        validate(&oversized, "claude"),
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn empty_provider_options_are_accepted_for_every_provider() {
    let empty = config(json!({"providerOptions":{}}));
    for provider in [
        "codex",
        "claude",
        "minimax-code",
        "deepseek-harness",
        "opencode",
    ] {
        assert_eq!(validate(&empty, provider), Ok(()), "{provider}");
    }
    assert_eq!(
        validate(
            &config(json!({"providerOptions":{"unknown":true}})),
            "minimax-code"
        ),
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn empty_and_inherited_configuration_does_not_add_cli_arguments() {
    assert_eq!(
        claude(&StoredAgentConfig::default(), false).unwrap(),
        [] as [String; 0]
    );
    assert_eq!(codex(&StoredAgentConfig::default()).unwrap(), json!({}));
    assert!(
        grants(&config(json!({"toolPolicy":null})))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn confined_claude_disables_its_nested_bash_sandbox_and_keeps_other_settings() {
    let config = config(json!({"providerOptions":{
        "settings":{"permissions":{"ask":["Edit"]}},
        "sandbox":{"enabled":true,"network":{"allowLocalBinding":true}}}}));

    let args = claude(&config, true).unwrap();

    let settings: Value = serde_json::from_str(
        args.iter()
            .find_map(|arg| arg.strip_prefix("--settings="))
            .expect("settings argument"),
    )
    .unwrap();
    assert_eq!(settings["sandbox"]["enabled"], false);
    assert_eq!(settings["sandbox"]["network"]["allowLocalBinding"], true);
    assert_eq!(settings["permissions"]["ask"], json!(["Edit"]));
    assert_eq!(
        claude(&StoredAgentConfig::default(), true).unwrap(),
        [r#"--settings={"sandbox":{"enabled":false}}"#.to_owned()]
    );
}

/// A `toolPolicy` that inherits no MCP servers, keeping `keep` by name.
fn strict(keep: &[&str]) -> Value {
    if keep.is_empty() {
        json!({"preapproved":[],"mcp":{"inherited":"none"}})
    } else {
        json!({"preapproved":[],"mcp":{"inherited":"none","keep":keep}})
    }
}

#[test]
fn claude_strict_mcp_limits_the_session_to_its_own_servers() {
    let strict_config = config(json!({"toolPolicy":strict(&[]),
        "mcpServers":{"bonsai_run":{"type":"http","url":"http://localhost:8860/mcp"}}}));
    let args = claude(&strict_config, false).unwrap();
    assert!(args.contains(&"--strict-mcp-config".to_owned()));
    // The servers themselves go through a private file (`claude_mcp`), never the arguments.
    assert!(claude_mcp(&strict_config).is_some());
    let alone = claude(&config(json!({"toolPolicy":strict(&[])})), false).unwrap();
    assert_eq!(alone, ["--strict-mcp-config"]);
    let default = claude(&config(json!({"toolPolicy":{"preapproved":[]}})), false).unwrap();
    assert!(!default.contains(&"--strict-mcp-config".to_owned()));
}

#[test]
fn the_mcp_scope_is_validated_with_the_rest_of_the_tool_policy() {
    for policy in [
        json!({"preapproved":[],"mcp":{"inherited":"all"}}),
        json!({"preapproved":[],"mcp":{"keep":["x"]}}),
        json!({"preapproved":[],"mcp":{"inherited":"none","keep":"x"}}),
        json!({"preapproved":[],"mcp":{"inherited":"none","keep":[1]}}),
        json!({"preapproved":[],"mcp":{"inherited":"none","other":true}}),
        json!({"preapproved":[],"other":{}}),
        json!({"mcp":{"inherited":"none"}}),
    ] {
        let config = config(json!({ "toolPolicy": policy }));
        assert!(validate(&config, "claude").is_err(), "{policy}");
        assert!(validate(&config, "codex").is_err(), "{policy}");
    }
    assert!(validate(&config(json!({"toolPolicy":strict(&["x"])})), "codex").is_ok());
}

#[test]
fn codex_strict_mcp_disables_inherited_servers_and_plugins_only() {
    assert!(!strict_mcp(&config(json!({}))));
    let config = config(json!({"toolPolicy":strict(&[]),
        "mcpServers":{"bonsai_run":{"type":"http","url":"http://localhost:8860/mcp"}}}));
    assert!(strict_mcp(&config));
    let mut native = codex(&config).unwrap();
    let effective = json!({"config":{
        "mcp_servers":{"bonsai_staging":{"url":"https://example.invalid/mcp","enabled":true},
                       "off":{"enabled":false}},
        "plugins":{"browser@openai-bundled":{"enabled":true},"idle@x":{"enabled":false}}}});
    codex_strict(&mut native, &effective, &config).unwrap();
    assert_eq!(
        native["mcp_servers"]["bonsai_staging"],
        json!({"enabled":false})
    );
    assert_eq!(native["mcp_servers"]["off"], json!({"enabled":false}));
    assert_eq!(
        native["mcp_servers"]["bonsai_run"]["url"],
        "http://localhost:8860/mcp"
    );
    assert_eq!(
        native["plugins"]["browser@openai-bundled"],
        json!({"enabled":false})
    );
    assert!(native["plugins"].get("idle@x").is_none());
    assert_eq!(native["features"]["apps"], false);
    assert_eq!(native["features"]["plugins"], false);
}

#[test]
fn codex_strict_mcp_refuses_a_user_server_named_like_the_sessions_own() {
    // Codex would merge the user's headers or command into the session's server.
    let config = config(json!({"toolPolicy":strict(&[]),
        "mcpServers":{"bonsai_run":{"type":"http","url":"http://localhost:8860/mcp"}}}));
    let mut native = codex(&config).unwrap();
    let effective = json!({"config":{"mcp_servers":{"bonsai_run":{
        "url":"https://example.invalid/mcp","bearer_token_env_var":"USER_TOKEN"}}}});
    assert_eq!(
        codex_strict(&mut native, &effective, &config),
        Err(AgentSessionError::Rejected)
    );
}

#[test]
fn codex_strict_mcp_keeps_only_the_named_inherited_servers() {
    let config = config(json!({"toolPolicy":strict(&["bonsai_staging"])}));
    let mut native = codex(&config).unwrap();
    let effective = json!({"config":{"mcp_servers":{
        "bonsai_staging":{"url":"https://example.invalid/mcp"},
        "gbrain":{"url":"http://localhost:3131/mcp"}}}});
    codex_strict(&mut native, &effective, &config).unwrap();
    assert!(native["mcp_servers"].get("bonsai_staging").is_none());
    assert_eq!(native["mcp_servers"]["gbrain"], json!({"enabled":false}));
}

#[test]
fn codex_mcp_inventory_lists_enabled_servers_with_their_addresses() {
    let effective = json!({"config":{"mcp_servers":{
        "bonsai_staging":{"url":"https://example.invalid/mcp","enabled":true},
        "off":{"url":"https://off.example.invalid/mcp","enabled":false},
        "local":{"command":"node"},
        "blank":{"url":""}}}});
    let mut servers = codex_mcp_servers(&effective);
    servers.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    assert_eq!(
        servers,
        [
            json!({"name":"blank","url":null,"type":"http","product":null}),
            json!({"name":"bonsai_staging","url":"https://example.invalid/mcp","type":"http","product":null}),
            json!({"name":"local","url":null,"type":"stdio","product":null}),
        ]
    );
    assert!(codex_mcp_servers(&json!({})).is_empty());
}

#[test]
fn claude_keeping_servers_denies_resources_and_checks_the_running_servers() {
    let keeping = config(json!({"toolPolicy":strict(&["plugin:bonsai:bonsai"]),
        "providerOptions":{"disallowedTools":["mcp__gbrain","ListMcpResourcesTool"]},
        "mcpServers":{"own":{"type":"http","url":"http://localhost:8860/mcp"}}}));
    assert!(claude_keeps(&keeping));
    let args = claude(&keeping, false).unwrap();
    assert!(!args.contains(&"--strict-mcp-config".to_owned()));
    assert!(args.contains(
        &"--disallowedTools=mcp__gbrain,ListMcpResourcesTool,ReadMcpResourceTool".to_owned()
    ));
    let status = |names: &[&str]| json!({"mcpServers": names.iter().map(|name| json!({"name": name})).collect::<Vec<_>>()});
    assert_eq!(
        claude_mcp_check(
            &keeping,
            &status(&["plugin:bonsai:bonsai", "gbrain", "own"])
        ),
        Ok(())
    );
    assert_eq!(
        claude_mcp_check(
            &keeping,
            &status(&["plugin:bonsai:bonsai", "claude.ai Gmail"])
        ),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        claude_mcp_check(&keeping, &json!({})),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        claude_server_rule("plugin:engineering:google calendar"),
        "mcp__plugin_engineering_google_calendar"
    );
    assert!(!claude_keeps(&config(json!({"toolPolicy":strict(&[])}))));
}
