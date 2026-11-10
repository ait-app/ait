//! Agent MCP injection through the private `--patch` overlay of each native Web Host.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::fixture::Fixture;
use crate::local::agent_mcp_injection::{Adapter, Opening, json_lines, verify};
use crate::local::deepseek_harness::DeepSeekHarnessClient;
use crate::ports::agent_session::{AgentClient, AgentSessionError};
use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Normalize each recorded overlay's inserted `dsh-mcp-client` rows.
fn openings(root: &Path) -> Vec<Opening> {
    json_lines(&root.join("patches.jsonl"))
        .iter()
        .map(|overlay| {
            overlay[0]["insert"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    assert_eq!(row["name"], "@deepseek-ai/dsh-mcp-client");
                    let config = &row["config"];
                    assert_eq!(config["transport"], "streamable-http");
                    (
                        config["serverName"].as_str().unwrap().to_owned(),
                        json!({"url":config["url"],"headers":config["headers"]}),
                    )
                })
                .collect()
        })
        .collect()
}

fn arguments(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("dsh.args"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Launcher that keeps the fixture's environment when the manager supplies an Agent's own.
fn managed_client(fixture: &Fixture) -> DeepSeekHarnessClient {
    use std::fmt::Write;
    let mut script = String::from("#!/bin/sh\n");
    for (name, value) in fixture.client.environment.entries() {
        writeln!(script, "export {name}='{value}'").unwrap();
    }
    let program = fixture.client.program.to_str().unwrap();
    writeln!(script, "exec '{program}' \"$@\"").unwrap();
    let launcher = fixture.root().join("managed-dsh");
    std::fs::write(&launcher, script).unwrap();
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
    DeepSeekHarnessClient::new(launcher)
}

#[tokio::test]
async fn host_overlay_receives_private_mcp_and_rebinds_after_restart() {
    let fixture = Fixture::new().await;
    let client = managed_client(&fixture);
    let build = || Box::new(client.clone()) as Box<dyn AgentClient>;
    let read = || openings(fixture.root());
    verify(&Adapter {
        client: &build,
        spec: fixture.spec.clone(),
        openings: &read,
        prompts: false,
    })
    .await;
}

#[tokio::test]
async fn overlay_is_owner_only_and_removed_with_the_host() {
    let fixture = Fixture::new().await;
    let mut spec = fixture.spec.clone();
    spec.config.mcp_servers = Some(BTreeMap::from([(
        "ait".into(),
        json!({"type":"http","url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}}),
    )]));
    let mut session = fixture.client.create_session(&spec).await.unwrap();
    let arguments = arguments(fixture.root());
    assert_eq!(
        arguments[..4],
        ["--profile", "web", "--patch", &arguments[3]]
    );
    assert_eq!(
        arguments[4], "--no-open",
        "launcher flags precede Host flags"
    );
    let overlay = PathBuf::from(&arguments[3]);
    assert_eq!(
        std::fs::metadata(&overlay).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        openings(fixture.root())[0]["ait"],
        json!({"url":"http://127.0.0.1:7316/mcp/agents",
            "headers":{"Authorization":"Bearer private","X-Ait-Agent-Id":"agent"}})
    );
    let mut changed = spec.config.clone();
    changed.mcp_servers = None;
    assert_eq!(
        session.validate_config_update(&changed),
        Err(AgentSessionError::Rejected)
    );
    assert_eq!(
        session.start_turn("hello", &changed).await,
        Err(AgentSessionError::Rejected)
    );
    assert!(
        !serde_json::to_string(&session.persistence())
            .unwrap()
            .contains("Bearer private")
    );
    session.close().await.unwrap();
    assert!(!overlay.exists(), "the overlay is removed with its Host");

    let plain = fixture.client.create_session(&fixture.spec).await;
    let mut plain = plain.unwrap();
    assert!(!arguments_contain_patch(fixture.root()));
    plain.close().await.unwrap();
}

fn arguments_contain_patch(root: &Path) -> bool {
    arguments(root).iter().any(|argument| argument == "--patch")
}

#[test]
fn native_mcp_names_and_transports_follow_the_plugin_schema() {
    for (name, server) in [
        (
            "has space",
            json!({"type":"http","url":"https://docs.test/mcp"}),
        ),
        (
            &*"n".repeat(33),
            json!({"type":"http","url":"https://docs.test/mcp"}),
        ),
        (
            "legacy",
            json!({"type":"sse","url":"https://docs.test/sse"}),
        ),
        ("relative", json!({"type":"stdio","command":"server"})),
    ] {
        let config = StoredAgentConfig {
            mcp_servers: Some(BTreeMap::from([(name.to_owned(), server)])),
            ..StoredAgentConfig::default()
        };
        assert_eq!(
            super::super::config::validate(&config),
            Err(AgentSessionError::Rejected),
            "{name}"
        );
    }
    let config = StoredAgentConfig {
        mcp_servers: Some(BTreeMap::from([(
            "local_tools-1".to_owned(),
            json!({"type":"stdio","command":"/usr/bin/server","args":["--stdio"]}),
        )])),
        ..StoredAgentConfig::default()
    };
    assert_eq!(super::super::config::validate(&config), Ok(()));
    let overlay = super::super::mcp::Overlay::write(&config).unwrap().unwrap();
    let [flag, path] = overlay.arguments();
    assert_eq!(flag, "--patch");
    let written: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        written,
        json!([{"insert":[{"id":"ait-session-mcp-local_tools-1",
            "name":"@deepseek-ai/dsh-mcp-client",
            "config":{"transport":"stdio","serverName":"local_tools-1",
                "command":"/usr/bin/server","args":["--stdio"]}}]}])
    );
    assert!(
        super::super::mcp::Overlay::write(&StoredAgentConfig::default())
            .unwrap()
            .is_none()
    );
}
