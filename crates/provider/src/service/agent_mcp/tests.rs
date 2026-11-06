use std::collections::BTreeMap;
use std::sync::Mutex;

use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use serde_json::Value;

use super::*;
use crate::ports::mcp_scope::McpAccess;

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

/// Fixed decision returned by [`FakeScopes`].
#[derive(Debug, Clone)]
pub(in crate::service) enum Grant {
    Shared,
    Scoped(String, bool),
    Denied,
}

/// Subject facts observed by [`FakeScopes::access`].
#[derive(Debug, Clone, PartialEq)]
pub(in crate::service) struct Seen {
    pub(in crate::service) internal: bool,
    pub(in crate::service) owner: Option<Value>,
    pub(in crate::service) labels: BTreeMap<String, String>,
}

/// Records every issue and release while returning a configurable decision.
#[derive(Debug)]
pub(in crate::service) struct FakeScopes {
    pub(in crate::service) grant: Mutex<Grant>,
    pub(in crate::service) calls: Mutex<Vec<String>>,
    pub(in crate::service) subjects: Mutex<Vec<Seen>>,
}

impl FakeScopes {
    pub(in crate::service) fn new(grant: Grant) -> Arc<Self> {
        Arc::new(Self {
            grant: Mutex::new(grant),
            calls: Mutex::default(),
            subjects: Mutex::default(),
        })
    }

    pub(in crate::service) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl McpScopes for FakeScopes {
    fn access(&self, subject: &McpSubject<'_>) -> McpAccess {
        self.calls
            .lock()
            .unwrap()
            .push(format!("access:{}", subject.agent_id));
        self.subjects.lock().unwrap().push(Seen {
            internal: subject.internal,
            owner: subject.owner.cloned(),
            labels: subject.labels.clone(),
        });
        match self.grant.lock().unwrap().clone() {
            Grant::Shared => McpAccess::Shared,
            Grant::Scoped(token, bot_session) => McpAccess::Scoped {
                token: token.into(),
                bot_session,
            },
            Grant::Denied => McpAccess::Denied,
        }
    }

    fn release(&self, agent_id: &str) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("release:{agent_id}"));
    }
}

fn subject<'a>(agent_id: &'a str, labels: &'a BTreeMap<String, String>) -> McpSubject<'a> {
    McpSubject {
        agent_id,
        internal: false,
        owner: None,
        labels,
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
            mcp_servers: Some(BTreeMap::from([(
                "docs".into(),
                json!({"type":"http","url":"https://example.test/mcp"}),
            )])),
            ..StoredAgentConfig::default()
        };
        let original = stored.clone();
        let labels = BTreeMap::new();
        let binding = source
            .bind(&subject("agent-a", &labels), &stored)
            .unwrap()
            .unwrap();
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
    let labels = BTreeMap::new();
    let binding = source
        .bind(&subject("a", &labels), &stored)
        .unwrap()
        .unwrap();
    for flags in [
        json!({"enabled":false,"injectIntoAgents":true}),
        json!({"enabled":true,"injectIntoAgents":false}),
    ] {
        *policy.0.lock().unwrap() = json!({"mcp":flags});
        assert!(
            source
                .bind(&subject("b", &labels), &stored)
                .unwrap()
                .is_none()
        );
        let mut effective = stored.clone();
        binding.apply(&mut effective);
        assert!(effective.mcp_servers.as_ref().unwrap().contains_key("ait"));
    }
    *policy.0.lock().unwrap() = Value::Null;
    assert_eq!(
        source.bind(&subject("b", &labels), &stored).unwrap_err(),
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
    let labels = BTreeMap::new();
    let binding = source
        .bind(&subject("a", &labels), &StoredAgentConfig::default())
        .unwrap()
        .unwrap();
    for server in [
        json!({"type":"stdio","command":"custom-ait"}),
        json!({"type":"http","url":"https://other-daemon.test/mcp/agents"}),
        json!({"type":"http","url":"http://127.0.0.1:8321/mcp/agents", "headers":{
            "X-Ait-Agent-Id":"user-selected-agent"}}),
    ] {
        let mut stored = StoredAgentConfig {
            mcp_servers: Some(BTreeMap::from([("ait".into(), server)])),
            ..StoredAgentConfig::default()
        };
        assert!(
            source
                .bind(&subject("a", &labels), &stored)
                .unwrap()
                .is_none()
        );
        let original = stored.clone();
        binding.apply(&mut stored);
        assert_eq!(stored, original);
    }
}

fn user_servers() -> StoredAgentConfig {
    StoredAgentConfig {
        mcp_servers: Some(BTreeMap::from([
            (
                "ait".into(),
                json!({"type":"http","url":"https://user.test/mcp"}),
            ),
            (
                "docs".into(),
                json!({"type":"http","url":"https://docs.test/mcp"}),
            ),
        ])),
        ..StoredAgentConfig::default()
    }
}

#[test]
fn scoped_and_denied_bindings_override_user_ait_entry_by_policy() {
    let credential = "s".repeat(64);
    let policy = Arc::new(Policy(Mutex::new(json!({}))));
    let scopes = FakeScopes::new(Grant::Shared);
    let source = AgentMcp::new(
        "127.0.0.1:7316".parse().unwrap(),
        TOKEN.into(),
        policy.clone(),
    )
    .with_scopes(scopes.clone());
    let stored = user_servers();
    let labels = BTreeMap::new();
    for (grant, flags, injected, released) in [
        (
            Grant::Scoped(credential.clone(), true),
            json!({"injectIntoAgents":false}),
            Some(true),
            false,
        ),
        (
            Grant::Scoped(credential.clone(), true),
            json!({"enabled":false}),
            Some(false),
            true,
        ),
        (
            Grant::Scoped(credential.clone(), false),
            json!({}),
            Some(true),
            false,
        ),
        (
            Grant::Scoped(credential.clone(), false),
            json!({"injectIntoAgents":false}),
            Some(false),
            true,
        ),
        (Grant::Denied, json!({}), Some(false), false),
        (Grant::Shared, json!({}), None, false),
    ] {
        *scopes.grant.lock().unwrap() = grant;
        *policy.0.lock().unwrap() = json!({ "mcp": flags });
        scopes.calls.lock().unwrap().clear();
        let binding = source.bind(&subject("a", &labels), &stored).unwrap();
        assert_eq!(
            scopes.calls(),
            if released {
                vec!["access:a".to_owned(), "release:a".to_owned()]
            } else {
                vec!["access:a".to_owned()]
            }
        );
        let mut effective = stored.clone();
        if let Some(binding) = &binding {
            assert!(!format!("{binding:?}").contains(&credential));
            binding.apply(&mut effective);
        }
        let servers = effective.mcp_servers.as_ref().unwrap();
        assert_eq!(
            servers["docs"],
            stored.mcp_servers.as_ref().unwrap()["docs"]
        );
        match injected {
            Some(true) => {
                assert_eq!(servers["ait"]["url"], "http://127.0.0.1:7316/mcp/agents");
                assert_eq!(
                    servers["ait"]["headers"]["Authorization"],
                    format!("Bearer {credential}")
                );
                assert_eq!(servers["ait"]["headers"]["X-Ait-Agent-Id"], "a");
            }
            Some(false) => assert!(!servers.contains_key("ait")),
            None => assert_eq!(servers["ait"], stored.mcp_servers.as_ref().unwrap()["ait"]),
        }
    }
}

#[test]
fn strip_removes_an_only_ait_entry_and_policy_failure_issues_nothing() {
    let mut only = StoredAgentConfig {
        mcp_servers: Some(BTreeMap::from([("ait".into(), json!({"type":"stdio"}))])),
        ..StoredAgentConfig::default()
    };
    Binding::Strip.apply(&mut only);
    assert_eq!(only, StoredAgentConfig::default());
    let scopes = FakeScopes::new(Grant::Scoped("s".repeat(64), true));
    let source = AgentMcp::new(
        "127.0.0.1:7316".parse().unwrap(),
        TOKEN.into(),
        Arc::new(Policy(Mutex::new(Value::Null))),
    )
    .with_scopes(scopes.clone());
    let labels = BTreeMap::new();
    assert_eq!(
        source
            .bind(&subject("a", &labels), &StoredAgentConfig::default())
            .unwrap_err(),
        model::ErrorCode::DaemonIo
    );
    assert!(scopes.calls().is_empty());
}
