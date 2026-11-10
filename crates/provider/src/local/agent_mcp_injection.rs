//! Shared Agent MCP injection checks driven through real adapters and their offline fakes.
//!
//! Each adapter supplies a client factory and a reader that returns, per native session
//! opening, the MCP servers the fake received as `name -> {"url", "headers"}`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domain::agent_runtime::registry::AgentRuntimeRegistry;
use domain::storage::daemon_config::{DaemonConfigReload, DaemonConfigStoreError};
use model::storage::daemon_config::DaemonConfigStore;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use serde_json::{Value, json};

use crate::ports::agent_session::{AgentClient, AgentSessionSpec};
use crate::ports::mcp_scope::{McpAccess, McpScopes, McpSubject};
use crate::service::agent_manager::{AgentManager, AgentRegistration};
use crate::service::agent_mcp::AgentMcp;

/// MCP servers one native opening received, keyed by server name.
pub(super) type Opening = BTreeMap<String, Value>;

/// Adapter under test and the observations its offline fake records.
pub(super) struct Adapter<'a> {
    /// Build a fresh client, as a daemon restart would.
    pub(super) client: &'a dyn Fn() -> Box<dyn AgentClient>,
    /// Interactive specification whose cwd hosts the fake's records.
    pub(super) spec: AgentSessionSpec,
    /// Read every native opening recorded so far, in order.
    pub(super) openings: &'a dyn Fn() -> Vec<Opening>,
    /// Whether the fake completes ordinary prompts; otherwise sessions are only opened.
    pub(super) prompts: bool,
}

#[derive(Debug)]
struct Policy(Mutex<Value>);

impl DaemonConfigStore for Policy {
    fn get(&self) -> Result<Value, DaemonConfigStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn patch(&self, _: &Value) -> Result<Value, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Invalid)
    }

    fn reload(&self) -> Result<DaemonConfigReload, DaemonConfigStoreError> {
        Err(DaemonConfigStoreError::Invalid)
    }
}

/// Scoped decision returned for every subject; releases are recorded.
#[derive(Debug)]
struct Scopes {
    token: Option<String>,
    released: Mutex<Vec<String>>,
}

impl McpScopes for Scopes {
    fn access(&self, _: &McpSubject<'_>) -> McpAccess {
        self.token
            .clone()
            .map_or(McpAccess::Denied, |token| McpAccess::Scoped {
                token: token.into(),
                bot_session: true,
            })
    }

    fn release(&self, agent_id: &str) {
        self.released.lock().unwrap().push(agent_id.to_owned());
    }
}

fn connection(port: u16, token: &str, policy: Value) -> AgentMcp {
    AgentMcp::new(
        format!("127.0.0.1:{port}").parse().unwrap(),
        token.to_owned().into(),
        Arc::new(Policy(Mutex::new(policy))),
    )
}

fn manager(
    adapter: &Adapter<'_>,
    registry: &FileBackedAgentRuntimeRegistry,
    mcp: AgentMcp,
) -> AgentManager {
    let mut manager = AgentManager::new(Box::new(registry.clone())).with_agent_mcp(mcp);
    manager.register_client((adapter.client)()).unwrap();
    manager
}

async fn turn(manager: &mut AgentManager, id: &str) {
    manager.send(id, "hello").await.unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        while manager.active_turn(id).is_some() {
            manager.poll().await.unwrap();
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn with_servers(spec: &AgentSessionSpec, user_ait: bool) -> AgentSessionSpec {
    let mut spec = spec.clone();
    let servers = spec.config.mcp_servers.get_or_insert_default();
    servers.insert(
        "docs".into(),
        json!({"type":"http","url":"https://docs.test/mcp","headers":{"X-Docs":"kept"}}),
    );
    if user_ait {
        servers.insert(
            "ait".into(),
            json!({"type":"http","url":"https://user.test/mcp",
                "headers":{"Authorization":"Bearer user"}}),
        );
    }
    spec
}

fn injected(port: u16, token: &str, agent: &str) -> Value {
    json!({"url":format!("http://127.0.0.1:{port}/mcp/agents"),
        "headers":{"Authorization":format!("Bearer {token}"),"X-Ait-Agent-Id":agent}})
}

fn user_ait() -> Value {
    json!({"url":"https://user.test/mcp","headers":{"Authorization":"Bearer user"}})
}

fn docs() -> Value {
    json!({"url":"https://docs.test/mcp","headers":{"X-Docs":"kept"}})
}

/// Openings recorded after the first `seen`, each of which must preserve the user's `docs`.
fn since(adapter: &Adapter<'_>, seen: usize) -> Vec<Opening> {
    let openings = (adapter.openings)().split_off(seen);
    assert!(!openings.is_empty(), "the native session was opened");
    for opening in &openings {
        assert_eq!(
            opening.get("docs"),
            Some(&docs()),
            "user MCP entries are kept"
        );
    }
    openings
}

/// Verify injection, precedence, policy, scoped credentials, history and rebinding.
///
/// Uses the shared token for ordinary sessions, rebinds it after a simulated restart and an
/// import, and asserts that no credential reaches the durable registry or native handle.
pub(super) async fn verify(adapter: &Adapter<'_>) {
    let root = tempfile::tempdir().unwrap();
    let registry = FileBackedAgentRuntimeRegistry::new(root.path().join("agents.json"));
    registry.initialize().unwrap();
    let first = uuid::Uuid::new_v4().to_string();
    let renewed = uuid::Uuid::new_v4().to_string();
    let spec = with_servers(&adapter.spec, false);

    let mut runtime = manager(adapter, &registry, connection(7316, &first, json!({})));
    runtime
        .create("agent-shared", &spec, AgentRegistration::default())
        .await
        .unwrap();
    if adapter.prompts {
        turn(&mut runtime, "agent-shared").await;
    }
    let launched = since(adapter, 0);
    assert_eq!(
        launched[0]["ait"],
        injected(7316, &first, "agent-shared"),
        "the opening receives the daemon address, MCP token and caller identity"
    );
    runtime.close("agent-shared").await.unwrap();

    let seen = (adapter.openings)().len();
    let mut runtime = manager(adapter, &registry, connection(8321, &renewed, json!({})));
    if adapter.prompts {
        turn(&mut runtime, "agent-shared").await;
    } else {
        runtime.resume("agent-shared").await.unwrap();
    }
    let resumed = since(adapter, seen);
    assert_eq!(resumed[0]["ait"], injected(8321, &renewed, "agent-shared"));
    runtime.close("agent-shared").await.unwrap();

    let seen = (adapter.openings)().len();
    let mut imported = registry.get("agent-shared").unwrap().unwrap();
    imported.id = "agent-imported".into();
    runtime.restore_new(imported).await.unwrap();
    let opened = since(adapter, seen);
    assert_eq!(opened[0]["ait"], injected(8321, &renewed, "agent-imported"));
    runtime.close("agent-imported").await.unwrap();

    let record = registry.get("agent-shared").unwrap().unwrap();
    assert_eq!(record.config.unwrap().mcp_servers, spec.config.mcp_servers);
    let durable = std::fs::read_to_string(root.path().join("agents.json")).unwrap();
    assert!(!durable.contains(&first) && !durable.contains(&renewed));

    verify_policy(adapter, &registry).await;
    verify_scoped(adapter, &registry).await;
    verify_history(adapter, &registry).await;
}

/// A user's own `ait` entry wins, and disabled injection leaves only the user's entries.
async fn verify_policy(adapter: &Adapter<'_>, registry: &FileBackedAgentRuntimeRegistry) {
    let token = uuid::Uuid::new_v4().to_string();
    let seen = (adapter.openings)().len();
    let mut runtime = manager(adapter, registry, connection(7316, &token, json!({})));
    runtime
        .create(
            "agent-user",
            &with_servers(&adapter.spec, true),
            AgentRegistration::default(),
        )
        .await
        .unwrap();
    runtime.close("agent-user").await.unwrap();
    assert_eq!(since(adapter, seen)[0]["ait"], user_ait());

    let seen = (adapter.openings)().len();
    let disabled = json!({"mcp":{"injectIntoAgents":false}});
    let mut runtime = manager(adapter, registry, connection(7316, &token, disabled));
    runtime
        .create(
            "agent-disabled",
            &with_servers(&adapter.spec, false),
            AgentRegistration::default(),
        )
        .await
        .unwrap();
    runtime.close("agent-disabled").await.unwrap();
    assert!(
        since(adapter, seen)
            .iter()
            .all(|opening| !opening.contains_key("ait"))
    );
}

/// Scoped sessions replace a user `ait` entry; undeliverable credentials remove it.
async fn verify_scoped(adapter: &Adapter<'_>, registry: &FileBackedAgentRuntimeRegistry) {
    let shared = uuid::Uuid::new_v4().to_string();
    let bot_token = "b".repeat(64);
    for (agent, token) in [
        ("agent-bot", Some(bot_token.clone())),
        ("agent-denied", None),
    ] {
        let scopes = Arc::new(Scopes {
            token: token.clone(),
            released: Mutex::default(),
        });
        let mcp = connection(7316, &shared, json!({})).with_scopes(scopes.clone());
        let seen = (adapter.openings)().len();
        let mut runtime = manager(adapter, registry, mcp);
        runtime
            .create(
                agent,
                &with_servers(&adapter.spec, true),
                AgentRegistration::default(),
            )
            .await
            .unwrap();
        runtime.close(agent).await.unwrap();
        let opened = since(adapter, seen);
        match token {
            Some(token) => {
                assert_eq!(opened[0]["ait"], injected(7316, &token, agent));
                assert_eq!(*scopes.released.lock().unwrap(), vec![agent.to_owned()]);
            }
            None => assert!(opened.iter().all(|opening| !opening.contains_key("ait"))),
        }
        assert!(
            !serde_json::to_string(&opened).unwrap().contains(&shared),
            "a scoped session never falls back to the shared token"
        );
    }
}

/// Read-only history sessions of archived Agents receive no daemon MCP.
async fn verify_history(adapter: &Adapter<'_>, registry: &FileBackedAgentRuntimeRegistry) {
    let token = uuid::Uuid::new_v4().to_string();
    let mut record = registry.get("agent-shared").unwrap().unwrap();
    record.archived_at = Some("2026-11-10T00:00:00Z".into());
    registry.upsert(&record).unwrap();
    let seen = (adapter.openings)().len();
    let mut runtime = manager(adapter, registry, connection(7316, &token, json!({})));
    // Adapters without native history replay refuse the read-only session instead.
    if runtime.resume("agent-shared").await.is_ok() {
        runtime.close("agent-shared").await.unwrap();
    }
    let openings = (adapter.openings)();
    assert!(
        openings[seen..]
            .iter()
            .all(|opening| !opening.contains_key("ait")),
        "history sessions are not injected"
    );
}

/// Normalize ACP `mcpServers` descriptions into [`Opening`] entries.
pub(super) fn acp_opening(servers: &Value) -> Opening {
    servers
        .as_array()
        .into_iter()
        .flatten()
        .map(|server| {
            let headers: serde_json::Map<String, Value> = server["headers"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|header| {
                    (
                        header["name"].as_str().unwrap().to_owned(),
                        header["value"].clone(),
                    )
                })
                .collect();
            (
                server["name"].as_str().unwrap().to_owned(),
                json!({"url":server["url"],"headers":headers}),
            )
        })
        .collect()
}

/// Read JSON lines from `path`, or none when the fake has not written it yet.
pub(super) fn json_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
