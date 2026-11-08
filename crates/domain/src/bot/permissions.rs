//! Bot permission profile: grantable MCP tool groups and the per-provider mode cap.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

const MAX_PROVIDERS: usize = 32;
const MAX_MODES: usize = 16;

/// Grantable MCP tool group; `Observe` is always granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    /// Read-only inspection; cannot be disabled.
    Observe,
    /// Create, prompt and reconfigure Agents.
    Agents,
    /// Cancel, archive and kill Agents.
    Stop,
    /// Manage schedules and heartbeats.
    Schedules,
    /// Start and stop Workspace scripts.
    Scripts,
    /// Create and drive terminals.
    Terminals,
    /// Retired: still parsed so saved profiles load, but grants nothing and is dropped on save.
    Workspaces,
}

/// How a tool may be reached by Bot-scoped callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAccess {
    /// Available when the group is granted.
    Group(ToolGroup),
    /// Visible only to Bot credentials, never to subtrees.
    BotOnly,
    /// Never granted to Bot-scoped callers.
    HostOnly,
}

use ToolAccess::{BotOnly, Group, HostOnly};
use ToolGroup::{Agents, Observe, Schedules, Scripts, Stop, Terminals};

/// Every daemon MCP tool name with its access class; unknown names are `HostOnly`.
pub const TOOLS: &[(&str, ToolAccess)] = &[
    ("list_agents", Group(Observe)),
    ("get_agent_status", Group(Observe)),
    ("get_agent_activity", Group(Observe)),
    ("wait_for_agent", Group(Observe)),
    ("list_schedules", Group(Observe)),
    ("inspect_schedule", Group(Observe)),
    ("schedule_logs", Group(Observe)),
    ("list_workspace_scripts", Group(Observe)),
    ("list_terminals", Group(Observe)),
    ("list_profiles", Group(Observe)),
    ("list_providers", Group(Observe)),
    ("list_models", Group(Observe)),
    ("inspect_provider", Group(Observe)),
    ("list_pending_permissions", Group(Observe)),
    ("create_agent", Group(Agents)),
    ("send_agent_prompt", Group(Agents)),
    ("update_agent", Group(Agents)),
    ("set_agent_mode", Group(Agents)),
    ("cancel_agent", Group(Stop)),
    ("archive_agent", Group(Stop)),
    ("kill_agent", Group(Stop)),
    ("create_schedule", Group(Schedules)),
    ("update_schedule", Group(Schedules)),
    ("pause_schedule", Group(Schedules)),
    ("resume_schedule", Group(Schedules)),
    ("run_schedule_once", Group(Schedules)),
    ("delete_schedule", Group(Schedules)),
    ("create_heartbeat", Group(Schedules)),
    ("delete_heartbeat", Group(Schedules)),
    ("start_workspace_script", Group(Scripts)),
    ("stop_workspace_script", Group(Scripts)),
    ("create_terminal", Group(Terminals)),
    ("send_terminal_keys", Group(Terminals)),
    ("capture_terminal", Group(Terminals)),
    ("kill_terminal", Group(Terminals)),
    ("bot_memory_read", BotOnly),
    ("bot_memory_update", BotOnly),
    ("workspace_overview", BotOnly),
    ("create_workspace", HostOnly),
    ("respond_to_permission", HostOnly),
    ("archive_workspace", HostOnly),
    ("rename_workspace", HostOnly),
    ("list_workspaces", HostOnly),
];

/// Return the access class for `tool`, failing closed to `HostOnly`.
#[must_use]
pub fn tool_access(tool: &str) -> ToolAccess {
    TOOLS
        .iter()
        .find_map(|(name, access)| (*name == tool).then_some(*access))
        .unwrap_or(HostOnly)
}

/// Tool names that may ever be granted to a Bot session (all except `HostOnly`).
pub fn grantable_tools() -> impl Iterator<Item = &'static str> {
    TOOLS
        .iter()
        .filter(|(_, access)| *access != HostOnly)
        .map(|(name, _)| *name)
}

/// Why a permission profile or mode request was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum PermissionError {
    /// The profile violates bounds or identifier rules.
    #[error("invalid permission profile")]
    Invalid,
    /// The provider has no allowed modes.
    #[error("provider is not allowed")]
    Provider,
    /// The mode is not in the provider's allowed list.
    #[error("mode is not allowed")]
    Mode,
}

/// User-configured Bot permission profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotPermissions {
    /// Granted tool groups.
    pub groups: BTreeSet<ToolGroup>,
    /// Native mode per provider for the Agents a Bot starts. Only the first entry applies; the
    /// list shape remains for profiles and clients that predate the single selector.
    pub modes: BTreeMap<String, Vec<String>>,
}

impl Default for BotPermissions {
    fn default() -> Self {
        Self {
            groups: BTreeSet::from([Observe, Agents, Stop, Schedules]),
            modes: BTreeMap::new(),
        }
    }
}

/// Same rules as `model::valid_id`, which `domain` cannot depend on.
fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

fn valid_modes(modes: &[String]) -> bool {
    (1..=MAX_MODES).contains(&modes.len())
        && modes.iter().all(|mode| valid_identifier(mode))
        && modes.iter().collect::<BTreeSet<_>>().len() == modes.len()
}

impl BotPermissions {
    /// Insert `Observe`, drop the retired group, validate bounds and identifiers, and keep only
    /// each provider's first mode.
    ///
    /// # Errors
    ///
    /// Returns `PermissionError::Invalid` when there are more than 32 providers, a provider
    /// has no or more than 16 modes, modes repeat, or an identifier is empty, longer than
    /// 128 bytes or contains control characters.
    pub fn normalized(mut self) -> Result<Self, PermissionError> {
        self.groups.insert(Observe);
        self.groups.remove(&ToolGroup::Workspaces);
        let valid = self.modes.len() <= MAX_PROVIDERS
            && self
                .modes
                .iter()
                .all(|(provider, modes)| valid_identifier(provider) && valid_modes(modes));
        if valid {
            for modes in self.modes.values_mut() {
                modes.truncate(1);
            }
            Ok(self)
        } else {
            Err(PermissionError::Invalid)
        }
    }

    /// Whether `tool` may be used by a caller; `bot` marks a Bot credential (not a subtree).
    #[must_use]
    pub fn allows(&self, tool: &str, bot: bool) -> bool {
        match tool_access(tool) {
            HostOnly => false,
            BotOnly => bot,
            Group(group) => group == Observe || self.groups.contains(&group),
        }
    }

    /// Resolve the mode for `provider`; `None` selects its configured mode.
    ///
    /// # Errors
    ///
    /// Returns `PermissionError::Provider` when the provider has no configured mode and
    /// `PermissionError::Mode` when `requested` differs from it.
    pub fn mode(&self, provider: &str, requested: Option<&str>) -> Result<String, PermissionError> {
        let configured = self
            .modes
            .get(provider)
            .and_then(|modes| modes.first())
            .ok_or(PermissionError::Provider)?;
        match requested {
            Some(mode) if mode != configured => Err(PermissionError::Mode),
            _ => Ok(configured.clone()),
        }
    }

    /// Observe-only profile used when a subtree has no current Bot.
    #[must_use]
    pub fn observe_only() -> Self {
        Self {
            groups: BTreeSet::from([Observe]),
            modes: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests;
