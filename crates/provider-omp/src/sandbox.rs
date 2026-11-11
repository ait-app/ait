//! OMP state a write-confined session keeps writing, and the configuration and code inside it
//! that stay read-only because later unconfined OMP processes obey or execute them (ADR-215,
//! as OMP 18.18.8 lays out its directories in `packages/utils/src/dirs.ts`).

use super::OmpClient;
use super::history::paths;
use provider::local::sandbox::StateLocation;

/// Agent-directory entries OMP obeys or executes: its settings (`config.yml`, the
/// `config.yaml` alternative and the legacy `settings.json` it migrates into them), custom
/// models (`models.yml`, `models.yaml` and the legacy `models.json`), user MCP servers, the
/// `.env` it loads into its environment, and extension, hook, custom tool and plugin code.
const AGENT_CONFIGURATION: &[&str] = &[
    ".env",
    ".mcp.json",
    "config.yaml",
    "config.yml",
    "extensions",
    "hooks",
    "mcp.json",
    "models.json",
    "models.yaml",
    "models.yml",
    "plugins",
    "settings.json",
    "tools",
];

/// [`AGENT_CONFIGURATION`] in the configuration root's default `agent` directory, plus the
/// root's own `.env`, installed plugins, the native addon OMP extracts and loads, and the
/// Python environment its tools run.
const ROOT_CONFIGURATION: &[&str] = &[
    ".env",
    "agent/.env",
    "agent/.mcp.json",
    "agent/config.yaml",
    "agent/config.yml",
    "agent/extensions",
    "agent/hooks",
    "agent/mcp.json",
    "agent/models.json",
    "agent/models.yaml",
    "agent/models.yml",
    "agent/plugins",
    "agent/settings.json",
    "agent/tools",
    "natives",
    "plugins",
    "python-env",
];

/// Code in an existing XDG data root, which then holds plugins, the native addon and the
/// Python environment.
const DATA_CONFIGURATION: &[&str] = &["natives", "plugins", "python-env"];

/// Every profile's configuration, kept read-only from a default-profile session.
const PROFILES: &[&str] = &["profiles"];

/// `~/.omp`, or `PI_CONFIG_DIR` below the home directory: logs, runtime state, caches and
/// the default profile's agent directory.
const CONFIG_ROOT: StateLocation =
    StateLocation::tree(Some("PI_CONFIG_DIR"), "", ".omp", ROOT_CONFIGURATION);

/// The agent directory `PI_CODING_AGENT_DIR` names: sessions, blobs and `agent.db`.
const AGENT_DIRECTORY: StateLocation = StateLocation::tree(
    Some("PI_CODING_AGENT_DIR"),
    "",
    ".omp/agent",
    AGENT_CONFIGURATION,
);

/// `$XDG_DATA_HOME/omp`, OMP's data root when it exists.
const DATA_ROOT: StateLocation = StateLocation::tree(
    Some("XDG_DATA_HOME"),
    "omp",
    ".local/share/omp",
    DATA_CONFIGURATION,
);

/// State of a named profile, whose `profiles/<name>` directories must stay writable.
static PROFILE_STATE: &[StateLocation] = &[CONFIG_ROOT, AGENT_DIRECTORY, DATA_ROOT];

/// State of the default profile, which never writes below `profiles`.
static DEFAULT_STATE: &[StateLocation] = &[
    CONFIG_ROOT,
    AGENT_DIRECTORY,
    DATA_ROOT,
    StateLocation::tree(Some("PI_CONFIG_DIR"), "", ".omp", PROFILES),
    StateLocation::tree(Some("XDG_DATA_HOME"), "omp", ".local/share/omp", PROFILES),
];

/// The state locations for `client`'s profile.
///
/// A named profile's own configuration lives below `profiles/<name>`, which a static
/// location cannot name, so it stays writable; an invalid profile makes OMP exit before it
/// writes anything.
pub(super) fn state(client: &OmpClient) -> &'static [StateLocation] {
    match paths::roots(client) {
        Ok(roots) if roots.profile.is_none() => DEFAULT_STATE,
        _ => PROFILE_STATE,
    }
}

#[cfg(test)]
mod tests;
