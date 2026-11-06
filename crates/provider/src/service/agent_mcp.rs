//! Session-private MCP configuration, separate from durable Agent settings.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use model::storage::daemon_config::DaemonConfigStore;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

use crate::ports::mcp_scope::{McpAccess, McpScopes, McpSubject};

/// Label carrying the share identity of an Agent created by a guest session.
///
/// Such Agents never receive the daemon MCP, which reaches every Workspace on this Host.
pub const GUEST_SHARE_LABEL: &str = "ait.guest.share-id";

/// Label carrying the visiting account of an Agent created by a guest session.
pub const GUEST_USER_LABEL: &str = "ait.guest.user-id";

/// Name of the injected daemon MCP server in a session's `mcp_servers` and `tool_policy`.
pub(crate) const SERVER: &str = "ait";

/// Host-owned connection facts and mutable injection policy shared by execution lanes.
#[derive(Debug, Clone)]
pub struct AgentMcp {
    url: String,
    token: SecretString,
    config: Arc<dyn DaemonConfigStore>,
    scopes: Option<Arc<dyn McpScopes>>,
}

/// Session-lifetime MCP decision, re-applied to every effective config (launch, inputs, config
/// updates); never serialized or persisted.
#[derive(Debug)]
pub(crate) enum Binding {
    /// Shared token; an existing user `ait` entry wins (ADR-202).
    Shared {
        url: String,
        token: SecretString,
        agent_id: String,
    },
    /// Scoped credential; always replaces any `ait` entry.
    Scoped {
        url: String,
        token: SecretString,
        agent_id: String,
    },
    /// No daemon MCP; always removes any `ait` entry.
    Strip,
}

impl AgentMcp {
    /// Construct a connection for an assigned local listener and its MCP-only credential.
    /// Wildcard listeners use a loopback client address. Policy is read at session opening.
    #[must_use]
    pub fn new(
        address: SocketAddr,
        token: SecretString,
        config: Arc<dyn DaemonConfigStore>,
    ) -> Self {
        let ip = match address.ip() {
            IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            ip => ip,
        };
        Self {
            url: format!("http://{}/mcp/agents", SocketAddr::new(ip, address.port())),
            token,
            config,
            scopes: None,
        }
    }

    /// Issue Bot-session and subtree credentials from `scopes` when sessions open.
    ///
    /// Without scopes every session uses the shared token.
    #[must_use]
    pub fn with_scopes(mut self, scopes: Arc<dyn McpScopes>) -> Self {
        self.scopes = Some(scopes);
        self
    }

    /// Decide the session's binding from the subject's credential and the current policy.
    ///
    /// `deliverable` is whether this session can receive the daemon MCP at all. `None` keeps
    /// the configuration unchanged: a shared session that is not deliverable, whose policy
    /// disables injection, or whose user configured an `ait` entry. Scoped and denied
    /// sessions that do not receive a scoped credential are stripped, and an issued
    /// credential that is not injected is revoked at once.
    pub(crate) fn bind(
        &self,
        subject: &McpSubject<'_>,
        config: &StoredAgentConfig,
        deliverable: bool,
    ) -> Result<Option<Binding>, model::ErrorCode> {
        let shared = deliverable && !has_ait(config);
        let Some(scopes) = &self.scopes else {
            return self.shared(subject.agent_id, shared);
        };
        match scopes.access(subject) {
            McpAccess::Shared => self.shared(subject.agent_id, shared),
            McpAccess::Denied => Ok(Some(Binding::Strip)),
            McpAccess::Scoped { token, bot_session } => {
                let policy = if deliverable {
                    self.policy()
                } else {
                    Ok((false, false))
                };
                match policy {
                    Ok((enabled, inject)) if enabled && (bot_session || inject) => {
                        Ok(Some(Binding::Scoped {
                            url: self.url.clone(),
                            token,
                            agent_id: subject.agent_id.to_owned(),
                        }))
                    }
                    policy => {
                        scopes.release(subject.agent_id);
                        policy.map(|_| Some(Binding::Strip))
                    }
                }
            }
        }
    }

    /// Revoke the scoped credential held by a session that left this manager.
    pub(crate) fn release(&self, agent_id: &str) {
        if let Some(scopes) = &self.scopes {
            scopes.release(agent_id);
        }
    }

    /// Bind the shared token when `candidate` and the policy enables injection.
    fn shared(&self, agent_id: &str, candidate: bool) -> Result<Option<Binding>, model::ErrorCode> {
        if !candidate || !self.policy()?.1 {
            return Ok(None);
        }
        Ok(Some(Binding::Shared {
            url: self.url.clone(),
            token: self.token.clone(),
            agent_id: agent_id.to_owned(),
        }))
    }

    /// Read `(mcp.enabled, mcp.enabled && mcp.injectIntoAgents)` at session opening.
    fn policy(&self) -> Result<(bool, bool), model::ErrorCode> {
        let policy = self.config.get().map_err(|_| model::ErrorCode::DaemonIo)?;
        let enabled = policy["mcp"]["enabled"] != false;
        Ok((
            enabled,
            enabled && policy["mcp"]["injectIntoAgents"] != false,
        ))
    }
}

impl Binding {
    /// Whether this binding holds a scoped credential that must be released with its session.
    pub(crate) const fn is_scoped(&self) -> bool {
        matches!(self, Self::Scoped { .. })
    }

    pub(crate) fn apply(&self, effective: &mut StoredAgentConfig) {
        let (url, token, agent_id) = match self {
            Self::Shared { .. } if has_ait(effective) => return,
            Self::Shared {
                url,
                token,
                agent_id,
            }
            | Self::Scoped {
                url,
                token,
                agent_id,
            } => (url, token, agent_id),
            Self::Strip => {
                if let Some(servers) = &mut effective.mcp_servers {
                    servers.remove(SERVER);
                    if servers.is_empty() {
                        effective.mcp_servers = None;
                    }
                }
                return;
            }
        };
        effective.mcp_servers.get_or_insert_default().insert(
            SERVER.to_owned(),
            json!({"type":"http","url":url,"headers":{
                "Authorization":format!("Bearer {}", token.expose_secret()),
                "X-Ait-Agent-Id":agent_id
            }}),
        );
    }
}

fn has_ait(config: &StoredAgentConfig) -> bool {
    config
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| servers.contains_key(SERVER))
}

#[cfg(test)]
pub(super) mod tests;
