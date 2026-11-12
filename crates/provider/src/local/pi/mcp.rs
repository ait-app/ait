//! Session-scoped MCP servers delivered to Pi through a private extension.
//!
//! Pi adds servers for one session with `pi.registerMcpServer(name, config)` from an extension
//! loaded by `--extension`; registrations are never saved to `mcp.json`. The definitions can
//! carry credentials, so they live in an owner-only file read at load time, beside an extension
//! whose source contains no secret, and the directory is removed with the native process.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use domain::agent_runtime::StoredAgentConfig;
use serde_json::{Map, Value, json};
use tempfile::TempDir;

use provider_core::ports::agent_session::AgentSessionError;

/// Extension module name; Pi loads it with jiti, which may cache this secret-free source.
const EXTENSION: &str = "ait-mcp.mjs";

/// Owner-only server definitions read by [`SOURCE`] on every extension load.
const SERVERS: &str = "servers.json";

/// Register every definition, then report the names Pi refused as one extension error.
const SOURCE: &str = r#"import { readFileSync } from "node:fs";

export default function (pi) {
    const servers = JSON.parse(readFileSync(new URL("./servers.json", import.meta.url), "utf8"));
    const refused = [];
    for (const [name, config] of Object.entries(servers)) {
        try {
            pi.registerMcpServer(name, config);
        } catch {
            refused.push(name);
        }
    }
    if (refused.length > 0) {
        throw new Error(`Pi refused Ait session MCP servers: ${refused.join(", ")}`);
    }
}
"#;

/// Private extension directory that must live as long as the native process.
#[derive(Debug)]
pub(super) struct Extension {
    directory: TempDir,
}

impl Extension {
    /// Write the extension for `config`'s MCP servers, or return `None` when there are none.
    ///
    /// # Errors
    ///
    /// Returns `Failed` when the private directory or its files cannot be written.
    pub(super) fn write(config: &StoredAgentConfig) -> Result<Option<Self>, AgentSessionError> {
        let Some(servers) = crate::local::acp_config::launched_servers(config) else {
            return Ok(None);
        };
        let servers: Map<String, Value> = servers
            .iter()
            .map(|(name, server)| (name.clone(), native(server)))
            .collect();
        let mut builder = tempfile::Builder::new();
        builder.prefix("ait-pi-mcp-");
        #[cfg(unix)]
        builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
        let directory = builder.tempdir().map_err(|_| AgentSessionError::Failed)?;
        let document = serde_json::to_vec(&servers).map_err(|_| AgentSessionError::Failed)?;
        private_file(directory.path(), SERVERS, &document)?;
        private_file(directory.path(), EXTENSION, SOURCE.as_bytes())?;
        Ok(Some(Self { directory }))
    }

    /// Native arguments that load this extension for one process.
    pub(super) fn arguments(&self) -> [OsString; 2] {
        [
            "--extension".into(),
            self.directory.path().join(EXTENSION).into_os_string(),
        ]
    }

    /// Directory holding the extension and its server definitions.
    #[cfg(test)]
    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
}

/// Map an Ait MCP definition to Pi's `mcpServers` entry shape.
///
/// Header and environment values are literal in Ait, while Pi resolves `$NAME`, `${NAME}` and
/// a leading `!command`; they are escaped so a value such as an Agent ID is never expanded.
fn native(server: &Value) -> Value {
    if server["type"] == "stdio" {
        let mut entry = json!({"command":server["command"]});
        if let Some(arguments) = server.get("args") {
            entry["args"] = arguments.clone();
        }
        if let Some(environment) = server.get("env") {
            entry["env"] = literals(environment);
        }
        return entry;
    }
    json!({"url":server["url"],"headers":server.get("headers").map_or_else(|| json!({}), literals)})
}

/// Escape every string value of a validated map with Pi's `$$` and `$!` literal forms.
fn literals(values: &Value) -> Value {
    values
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, value)| {
            let text = value.as_str().unwrap_or_default().replace('$', "$$");
            let text = if text.starts_with('!') {
                format!("${text}")
            } else {
                text
            };
            (name.clone(), Value::String(text))
        })
        .collect::<Map<String, Value>>()
        .into()
}

/// Create `name` in `directory` with owner-only permissions and the complete `contents`.
fn private_file(directory: &Path, name: &str, contents: &[u8]) -> Result<(), AgentSessionError> {
    let mut file =
        tempfile::NamedTempFile::new_in(directory).map_err(|_| AgentSessionError::Failed)?;
    file.write_all(contents)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| AgentSessionError::Failed)?;
    file.persist(directory.join(name))
        .map_err(|_| AgentSessionError::Failed)?;
    Ok(())
}
