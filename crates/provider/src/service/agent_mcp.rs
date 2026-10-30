//! Session-private MCP configuration, separate from durable Agent settings.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use domain::agent_runtime::StoredAgentConfig;
use model::storage::daemon_config::DaemonConfigStore;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

/// Label carrying the share identity of an Agent created by a guest session.
///
/// Such Agents never receive the daemon MCP, which reaches every Workspace on this Host.
pub const GUEST_SHARE_LABEL: &str = "ait.guest.share-id";

/// Label carrying the visiting account of an Agent created by a guest session.
pub const GUEST_USER_LABEL: &str = "ait.guest.user-id";

/// Host-owned connection facts and mutable injection policy shared by execution lanes.
#[derive(Debug, Clone)]
pub struct AgentMcp {
    url: String,
    token: SecretString,
    config: Arc<dyn DaemonConfigStore>,
}

/// Connection captured when opening a native session; never serialized or persisted.
#[derive(Debug, Clone)]
pub(crate) struct Binding {
    url: String,
    token: SecretString,
    agent_id: String,
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
        }
    }

    pub(crate) fn bind(
        &self,
        agent_id: &str,
        config: &StoredAgentConfig,
    ) -> Result<Option<Binding>, model::ErrorCode> {
        if config
            .mcp_servers
            .as_ref()
            .is_some_and(|servers| servers.contains_key("ait"))
        {
            return Ok(None);
        }
        let policy = self.config.get().map_err(|_| model::ErrorCode::DaemonIo)?;
        if policy["mcp"]["enabled"] == false || policy["mcp"]["injectIntoAgents"] == false {
            return Ok(None);
        }
        Ok(Some(Binding {
            url: self.url.clone(),
            token: self.token.clone(),
            agent_id: agent_id.to_owned(),
        }))
    }
}

impl Binding {
    pub(crate) fn apply(&self, effective: &mut StoredAgentConfig) {
        if effective
            .mcp_servers
            .as_ref()
            .is_some_and(|servers| servers.contains_key("ait"))
        {
            return;
        }
        effective.mcp_servers.get_or_insert_default().insert(
            "ait".to_owned(),
            json!({"type":"http","url":self.url,"headers":{
                "Authorization":format!("Bearer {}", self.token.expose_secret()),
                "X-Ait-Agent-Id":self.agent_id
            }}),
        );
    }
}

#[cfg(test)]
pub(super) mod tests;
