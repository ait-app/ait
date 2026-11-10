//! Session-scoped MCP servers delivered to one private Web Host through a `--patch` overlay.
//!
//! DSH composes a profile from patch layers and applies `--patch` overlays last, for that
//! process only. Each server becomes one inserted `@deepseek-ai/dsh-mcp-client` plugin row.
//! The overlay can carry credentials, so it is an owner-only temporary file removed with the
//! Host process; the profile's own `cordis.patch.yml` is never written.

use std::ffi::OsString;
use std::io::Write;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Value, json};
use tempfile::NamedTempFile;

use crate::ports::agent_session::AgentSessionError;

/// Plugin package implementing stdio and Streamable HTTP MCP clients.
const PLUGIN: &str = "@deepseek-ai/dsh-mcp-client";

/// Prefix of inserted plugin row identities, distinct from profile-owned rows.
const ROW: &str = "ait-session-mcp-";

/// Whether `name` is a valid `dsh-mcp-client` `serverName` (`[A-Za-z0-9_-]{1,32}`).
pub(super) fn server_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Private overlay file that must live as long as the Host process.
#[derive(Debug)]
pub(super) struct Overlay {
    file: NamedTempFile,
}

impl Overlay {
    /// Write the overlay for `config`'s MCP servers, or return `None` when there are none.
    ///
    /// # Errors
    ///
    /// Returns `Failed` when the private file cannot be written.
    pub(super) fn write(config: &StoredAgentConfig) -> Result<Option<Self>, AgentSessionError> {
        let Some(servers) = crate::local::acp_config::launched_servers(config) else {
            return Ok(None);
        };
        let rows: Vec<Value> = servers
            .iter()
            .map(|(name, server)| {
                json!({"id":format!("{ROW}{name}"),"name":PLUGIN,"config":plugin(name, server)})
            })
            .collect();
        // JSON is YAML: quoted scalars cannot carry the loader's `!!js` tags.
        let document =
            serde_json::to_vec(&json!([{"insert":rows}])).map_err(|_| AgentSessionError::Failed)?;
        let mut file = tempfile::Builder::new()
            .prefix("ait-dsh-mcp-")
            .suffix(".yml")
            .tempfile()
            .map_err(|_| AgentSessionError::Failed)?;
        file.write_all(&document)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| AgentSessionError::Failed)?;
        Ok(Some(Self { file }))
    }

    /// Launcher arguments applying this overlay; they precede the Web Host's own flags.
    pub(super) fn arguments(&self) -> [OsString; 2] {
        ["--patch".into(), self.file.path().as_os_str().to_owned()]
    }
}

/// Map an Ait MCP definition to the `dsh-mcp-client` plugin configuration.
fn plugin(name: &str, server: &Value) -> Value {
    if server["type"] == "stdio" {
        let mut config = json!({"transport":"stdio","serverName":name,"command":server["command"]});
        for field in ["args", "env"] {
            if let Some(value) = server.get(field) {
                config[field] = value.clone();
            }
        }
        return config;
    }
    json!({"transport":"streamable-http","serverName":name,"url":server["url"],
        "headers":server.get("headers").cloned().unwrap_or_else(|| json!({}))})
}
