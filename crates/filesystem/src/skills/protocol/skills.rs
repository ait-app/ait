//! Skill selection, installation status and canonical requests.

/// Canonical get status request.
pub(crate) const GET_STATUS: &str = "agent.skills.get_status.request";
/// Canonical reconcile request.
pub(crate) const RECONCILE: &str = "agent.skills.reconcile.request";
/// Canonical uninstall request.
pub(crate) const UNINSTALL: &str = "agent.skills.uninstall.request";
/// Canonical save selection request.
pub(crate) const SAVE_SELECTION: &str = "agent.skills.save_selection.request";
/// Canonical import legacy selection request.
pub(crate) const IMPORT_LEGACY_SELECTION: &str = "agent.skills.import_legacy_selection.request";

/// Desired bundle membership; unknown custom names are retained but not installed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
pub enum Selection {
    /// Follow every current and future bundled skill.
    All {},
    /// Install the named subset of the bundle.
    Custom {
        /// Normalized, sorted and deduplicated names.
        skills: Vec<String>,
    },
}

impl Default for Selection {
    fn default() -> Self {
        Self::All {}
    }
}

impl Selection {
    /// Normalize UI-provided names without treating names as filesystem paths.
    pub(crate) fn normalize(&mut self) {
        if let Self::Custom { skills } = self {
            *skills = skills
                .iter()
                .map(|name| name.trim().to_owned())
                .filter(|name| !name.is_empty())
                .collect();
            skills.sort();
            skills.dedup();
        }
    }
    /// Whether a catalogued skill is selected.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        match self {
            Self::All {} => true,
            Self::Custom { skills } => skills.iter().any(|skill| skill == name),
        }
    }
}

/// Change needed for one managed skill across all three agent homes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Operation {
    /// Add, update or delete.
    pub(crate) kind: Kind,
    /// A catalogued or legacy managed directory name.
    pub(crate) name: String,
}

/// Convergence action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    /// Missing from at least one target.
    Add,
    /// A bundled file differs on a target.
    Update,
    /// A managed directory is no longer selected.
    Delete,
}

/// Paseo settings snapshot, with deterministic lexical ordering.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// not-installed, up-to-date or drift.
    pub(crate) state: String,
    /// Pending changes.
    pub(crate) ops: Vec<Operation>,
    /// Current bundle catalog.
    pub(crate) available: Vec<String>,
    /// Managed names present on at least one target.
    pub(crate) installed: Vec<String>,
    /// Persisted desired selection.
    pub(crate) selection: Selection,
}

/// Save request; deletion consent applies to this operation's fresh plan.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SaveRequest {
    /// Desired selection.
    pub(crate) selection: Selection,
    /// Explicitly confirmed managed directory removals.
    #[serde(default)]
    pub(crate) confirmed_removals: Vec<String>,
}

/// Import only if no server-owned selection exists.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportRequest {
    /// Legacy client preference.
    pub(crate) selection: Selection,
}
