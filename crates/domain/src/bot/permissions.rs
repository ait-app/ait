//! Bot permission profile: grantable MCP tool groups and the per-provider mode cap.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

const MAX_PROVIDERS: usize = 32;
const MAX_MODES: usize = 16;

/// Grantable MCP tool group; `Observe` is always granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    /// Inspection and the Workspace notepad; cannot be disabled.
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
    /// Visible only to subtree credentials of Agents working on a Bot ticket, never to Bot
    /// sessions or the host.
    Worker,
    /// Never granted to Bot-scoped callers.
    HostOnly,
}

/// Which kind of scoped credential asks for a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// A Bot's own conversation session.
    Bot,
    /// An Agent started inside a Bot's scope, or one of its descendants.
    Subtree,
    /// A subtree Agent whose `ait.bot.ticket` label assigns it to one of the Bot's tickets.
    Worker,
}

use ToolAccess::{BotOnly, Group, HostOnly, Worker};
use ToolGroup::{Agents, Observe, Schedules, Scripts, Stop, Terminals};

/// Every daemon MCP tool name with its access class; unknown names are `HostOnly`.
pub const TOOLS: &[(&str, ToolAccess)] = &[
    ("agent_list", Group(Observe)),
    ("agent_get", Group(Observe)),
    ("agent_activity_read", Group(Observe)),
    ("agent_wait", Group(Observe)),
    ("schedule_list", Group(Observe)),
    ("schedule_get", Group(Observe)),
    ("schedule_log_read", Group(Observe)),
    ("script_list", Group(Observe)),
    ("script_run_get", Group(Observe)),
    ("script_run_wait", Group(Observe)),
    ("terminal_list", Group(Observe)),
    ("profile_list", Group(Observe)),
    ("provider_list", Group(Observe)),
    ("provider_model_list", Group(Observe)),
    ("provider_get", Group(Observe)),
    ("permission_list", Group(Observe)),
    ("agent_create", Group(Agents)),
    ("agent_prompt", Group(Agents)),
    ("agent_update", Group(Agents)),
    ("agent_mode_set", Group(Agents)),
    ("agent_cancel", Group(Stop)),
    ("agent_archive", Group(Stop)),
    ("schedule_create", Group(Schedules)),
    ("schedule_update", Group(Schedules)),
    ("schedule_pause", Group(Schedules)),
    ("schedule_resume", Group(Schedules)),
    ("schedule_run", Group(Schedules)),
    ("schedule_delete", Group(Schedules)),
    ("heartbeat_create", Group(Schedules)),
    ("heartbeat_delete", Group(Schedules)),
    ("heartbeat_list", Group(Observe)),
    // The notepad belongs to the Workspace, so every scoped caller that reaches the Workspace
    // may read and write it (ADR-296).
    ("notepad_read", Group(Observe)),
    ("notepad_write", Group(Observe)),
    ("script_start", Group(Scripts)),
    ("script_stop", Group(Scripts)),
    ("terminal_create", Group(Terminals)),
    ("terminal_send", Group(Terminals)),
    ("terminal_capture", Group(Terminals)),
    ("terminal_kill", Group(Terminals)),
    ("bot_memory_read", BotOnly),
    ("bot_memory_update", BotOnly),
    ("bot_overview", BotOnly),
    ("bot_ticket_list", BotOnly),
    ("bot_ticket_update", BotOnly),
    ("bot_read", BotOnly),
    ("ticket_report", Worker),
    ("ticket_read", Worker),
    ("workspace_create", HostOnly),
    ("permission_respond", HostOnly),
    ("workspace_archive", HostOnly),
    ("workspace_rename", HostOnly),
    ("workspace_list", HostOnly),
];

/// Return the access class for `tool`, failing closed to `HostOnly`.
#[must_use]
pub fn tool_access(tool: &str) -> ToolAccess {
    TOOLS
        .iter()
        .find_map(|(name, access)| (*name == tool).then_some(*access))
        .unwrap_or(HostOnly)
}

/// Tool names that may ever be granted to a Bot session (all except `HostOnly` and `Worker`).
pub fn grantable_tools() -> impl Iterator<Item = &'static str> {
    TOOLS
        .iter()
        .filter(|(_, access)| !matches!(access, HostOnly | Worker))
        .map(|(name, _)| *name)
}

/// Tool names only an Agent working on a Bot ticket may call (ADR-274, ADR-280).
pub fn worker_tools() -> impl Iterator<Item = &'static str> {
    TOOLS
        .iter()
        .filter(|(_, access)| matches!(access, Worker))
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

    /// Whether `tool` may be used by a scoped credential of kind `holder`.
    #[must_use]
    pub fn allows(&self, tool: &str, holder: Holder) -> bool {
        match tool_access(tool) {
            HostOnly => false,
            BotOnly => holder == Holder::Bot,
            Worker => holder == Holder::Worker,
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
