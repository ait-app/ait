use std::sync::Mutex;

use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use serde_json::Value;

use super::*;

pub(in crate::service) const TOKEN: &str = "offline-agent-mcp-credential-at-least-32-characters";

#[derive(Debug)]
pub(in crate::service) struct Policy(pub(in crate::service) Mutex<Value>);

impl DaemonConfigStore for Policy {
    fn get(&self) -> Result<Value, DaemonConfigStoreError> {
        let value = self.0.lock().unwrap().clone();
        if value.is_null() {
            Err(DaemonConfigStoreError::Io)
        } else {
            Ok(value)
        }
    }

    fn patch(&self, _: &Value) -> Result<Value, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Invalid)
    }

    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Invalid)
    }
}

#[test]
fn connection_maps_listener_and_preserves_user_configuration() {
    for (listen, expected) in [
        ("0.0.0.0:7316", "http://127.0.0.1:7316/mcp/agents"),
        ("[::]:7317", "http://[::1]:7317/mcp/agents"),
        ("192.0.2.1:7318", "http://192.0.2.1:7318/mcp/agents"),
    ] {
        let source = AgentMcp::new(
            listen.parse().unwrap(),
            TOKEN.into(),
            Arc::new(Policy(Mutex::new(json!({})))),
        );
        let stored = StoredAgentConfig {
            mcp_servers: Some(std::collections::BTreeMap::from([(
                "docs".into(),
                json!({"type":"http","url":"https://example.test/mcp"}),
            )])),
            ..StoredAgentConfig::default()
        };
        let original = stored.clone();
        let binding = source.bind("agent-a", &stored).unwrap().unwrap();
        let mut effective = stored.clone();
        binding.apply(&mut effective);
        let servers = effective.mcp_servers.as_ref().unwrap();
        assert_eq!(servers["ait"]["url"], expected);
        assert_eq!(
            servers["ait"]["headers"]["Authorization"],
            format!("Bearer {TOKEN}")
        );
        assert_eq!(servers["ait"]["headers"]["X-Ait-Agent-Id"], "agent-a");
        assert_eq!(
            servers["docs"],
            original.mcp_servers.as_ref().unwrap()["docs"]
        );
        assert_eq!(stored, original);
        assert!(!format!("{source:?} {binding:?}").contains(TOKEN));
    }
}

#[test]
fn flags_are_read_at_opening_and_existing_bindings_remain_stable() {
    let policy = Arc::new(Policy(Mutex::new(
        json!({"mcp":{"enabled":true,"injectIntoAgents":true}}),
    )));
    let source = AgentMcp::new(
        "127.0.0.1:7316".parse().unwrap(),
        TOKEN.into(),
        policy.clone(),
    );
    let stored = StoredAgentConfig::default();
    let binding = source.bind("a", &stored).unwrap().unwrap();
    for flags in [
        json!({"enabled":false,"injectIntoAgents":true}),
        json!({"enabled":true,"injectIntoAgents":false}),
    ] {
        *policy.0.lock().unwrap() = json!({"mcp":flags});
        assert!(source.bind("b", &stored).unwrap().is_none());
        let mut effective = stored.clone();
        binding.apply(&mut effective);
        assert!(effective.mcp_servers.as_ref().unwrap().contains_key("ait"));
    }
    *policy.0.lock().unwrap() = Value::Null;
    assert_eq!(
        source.bind("b", &stored).unwrap_err(),
        model::ErrorCode::DaemonIo
    );
}

#[test]
fn explicit_ait_server_is_never_replaced() {
    let source = AgentMcp::new(
        "127.0.0.1:7316".parse().unwrap(),
        TOKEN.into(),
        Arc::new(Policy(Mutex::new(json!({})))),
    );
    let binding = source
        .bind("a", &StoredAgentConfig::default())
        .unwrap()
        .unwrap();
    for server in [
        json!({"type":"stdio","command":"custom-ait"}),
        json!({"type":"http","url":"https://other-daemon.test/mcp/agents"}),
        json!({"type":"http","url":"http://127.0.0.1:8321/mcp/agents", "headers":{
            "X-Ait-Agent-Id":"user-selected-agent"}}),
    ] {
        let mut stored = StoredAgentConfig {
            mcp_servers: Some(std::collections::BTreeMap::from([("ait".into(), server)])),
            ..StoredAgentConfig::default()
        };
        assert!(source.bind("a", &stored).unwrap().is_none());
        let original = stored.clone();
        binding.apply(&mut stored);
        assert_eq!(stored, original);
    }
}
