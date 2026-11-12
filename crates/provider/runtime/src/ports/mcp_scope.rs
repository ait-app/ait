//! Scoped Ait MCP credentials chosen when a native session opens.
//!
//! The registry is owned by the Bot domain; Provider only asks for a decision and revokes the
//! credential when the session it was issued for ends.

use std::collections::BTreeMap;

use secrecy::SecretString;
use serde_json::Value;

/// Session facts read from a registration or persisted record when opening a native session.
#[derive(Debug, Clone, Copy)]
pub struct McpSubject<'a> {
    /// Host Agent identity of the session being opened.
    pub agent_id: &'a str,
    /// Whether the session is a hidden internal session.
    pub internal: bool,
    /// Opaque internal owner, such as a Bot `SessionOwner`.
    pub owner: Option<&'a Value>,
    /// Agent labels, including the reserved Bot scope label.
    pub labels: &'a BTreeMap<String, String>,
}

/// Credential decision for one native session opening.
#[derive(Debug)]
pub enum McpAccess {
    /// Existing shared Agent MCP token and precedence rules.
    Shared,
    /// Scoped credential; `bot_session` sessions ignore `injectIntoAgents`.
    Scoped {
        /// Bearer credential, valid only in daemon memory until released or superseded.
        token: SecretString,
        /// Whether the credential belongs to a Bot's own hidden session.
        bot_session: bool,
        /// Whether the credential belongs to an Agent working on a Bot ticket, whose report
        /// tool adapters with additive pre-approval run without asking.
        worker: bool,
    },
    /// Inject no `ait` server and remove any configured `ait` entry.
    Denied,
}

/// Issues and revokes scoped Ait MCP credentials for native sessions.
pub trait McpScopes: std::fmt::Debug + Send + Sync {
    /// Decide the credential for a session opening; a new credential supersedes the
    /// previous one issued for the same Agent.
    fn access(&self, subject: &McpSubject<'_>) -> McpAccess;
    /// Revoke the credential issued for `agent_id`, if any; idempotent.
    fn release(&self, agent_id: &str);
}
