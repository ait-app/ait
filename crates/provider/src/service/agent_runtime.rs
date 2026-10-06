//! Durable Paseo Agent runtime directory and metadata lifecycle use cases.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::DateTime;
use domain::agent_runtime::{
    AgentAttentionReason, AgentRuntimeStatus, PersistedAgentRuntimeRecord,
};
use metadata::model::registry::{
    PersistedProjectKind, PersistedProjectRecord, PersistedWorkspaceKind, PersistedWorkspaceRecord,
};
use metadata::ports::registry::{ProjectRegistry, RegistryError, WorkspaceRegistry};
use model::pagination::{self, Direction, Entry, Sort, SortValue};

use crate::ports::agent_runtime::{AgentRuntimeRegistry, AgentRuntimeRegistryError};

pub(crate) mod archive;
mod search;

const PARENT_AGENT_ID_LABEL: &str = "paseo.parent-agent-id";
const OPEN_AGENT_TAB_LABEL_PREFIX: &str = "paseo.open-agent-tab.";
const DEFAULT_PAGE_LIMIT: usize = 200;

/// Sortable Agent directory fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSortKey {
    /// Attention and lifecycle priority.
    StatusPriority,
    /// Creation timestamp.
    CreatedAt,
    /// Update timestamp.
    UpdatedAt,
    /// Case-insensitive title.
    Title,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending order.
    Asc,
    /// Descending order.
    Desc,
}

/// One ordered sort term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSort {
    /// Sort field.
    pub key: AgentSortKey,
    /// Sort direction.
    pub direction: SortDirection,
}

/// Application-level Agent directory query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentDirectoryQuery {
    /// Require active workspace/project placement.
    pub active_scope: bool,
    /// Include archived Agent records.
    pub include_archived: bool,
    /// Exact label filters.
    pub labels: BTreeMap<String, String>,
    /// Allowed project keys.
    pub project_keys: Option<BTreeSet<String>>,
    /// Allowed lifecycle statuses.
    pub statuses: Option<BTreeSet<AgentRuntimeStatus>>,
    /// Required attention value.
    pub requires_attention: Option<bool>,
    /// Configured thinking option filter, when one was requested.
    pub thinking_option_id: Option<ThinkingOptionFilter>,
    /// Case-insensitive history search.
    pub search: Option<String>,
    /// Ordered sort fields.
    pub sort: Vec<AgentSort>,
    /// Opaque keyset boundary from the preceding page.
    pub cursor: Option<String>,
    /// Requested page size.
    pub limit: usize,
}

/// Requested thinking option for one Agent directory search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThinkingOptionFilter {
    /// Match the provider's default option.
    ProviderDefault,
    /// Match a specific option identifier.
    Selected(String),
}

/// Placement facts required by the Paseo Agent directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPlacement {
    /// Whether both workspace and project remain active.
    pub active: bool,
    /// Project key.
    pub project_key: String,
    /// Current project display name.
    pub project_name: String,
    /// Current workspace display name.
    pub workspace_name: String,
    /// Selected working directory.
    pub cwd: String,
    /// Whether placement is Git-backed.
    pub is_git: bool,
    /// Stored branch identity.
    pub current_branch: Option<String>,
    /// Stored checkout root.
    pub worktree_root: Option<String>,
    /// Whether Paseo owns the linked worktree.
    pub is_paseo_owned_worktree: bool,
    /// Main repository root for a managed worktree.
    pub main_repo_root: Option<String>,
}

/// One Agent directory row before transport projection.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentDirectoryEntry {
    /// Durable Agent runtime snapshot.
    pub agent: PersistedAgentRuntimeRecord,
    /// Project/workspace placement.
    pub placement: AgentPlacement,
}

/// One page of Agent directory rows.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentDirectoryPage {
    /// Matching rows.
    pub entries: Vec<AgentDirectoryEntry>,
    /// Keyset cursor for the next page.
    pub next_cursor: Option<String>,
    /// Cursor used for this page.
    pub prev_cursor: Option<String>,
    /// Whether another matching page exists.
    pub has_more: bool,
}

/// Agent lookup result with optional placement.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAgent {
    /// Durable Agent runtime snapshot.
    pub agent: PersistedAgentRuntimeRecord,
    /// Placement when its workspace and project still exist.
    pub placement: Option<AgentPlacement>,
}

/// Return the newer valid timestamp from durable metadata and provider activity.
#[must_use]
pub fn resolved_updated_at(record: &PersistedAgentRuntimeRecord) -> &str {
    let updated = parse_timestamp(&record.updated_at);
    let activity = record.last_activity_at.as_deref().and_then(parse_timestamp);
    if activity.is_some_and(|activity| updated.is_none_or(|updated| activity > updated)) {
        record
            .last_activity_at
            .as_deref()
            .unwrap_or(&record.updated_at)
    } else {
        &record.updated_at
    }
}

/// Agent runtime application failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentRuntimeError {
    /// Query parameters are outside the supported bounds.
    #[error("invalid Agent directory request")]
    InvalidRequest,
    /// The requested Agent is missing.
    #[error("Agent not found: {0}")]
    NotFound(String),
    /// A prefix or title resolves to several Agents.
    #[error("Agent identifier is ambiguous: {0}")]
    Ambiguous(String),
    /// Runtime record persistence failed.
    #[error("Agent runtime registry failed")]
    AgentRegistry,
    /// Project/workspace placement persistence failed.
    #[error("workspace registry failed")]
    WorkspaceRegistry,
}

/// Durable Agent runtime directory independent of provider execution.
#[derive(Debug, Clone)]
pub struct AgentRuntimeDirectory {
    pub(crate) sync: model::directory_sync::DirectorySync,
    agents: Arc<dyn AgentRuntimeRegistry>,
    workspaces: Arc<dyn WorkspaceRegistry>,
    projects: Arc<dyn ProjectRegistry>,
}

impl AgentRuntimeDirectory {
    /// Compose the runtime snapshot and placement registries.
    #[must_use]
    pub fn new(
        agents: Box<dyn AgentRuntimeRegistry>,
        workspaces: Box<dyn WorkspaceRegistry>,
        projects: Box<dyn ProjectRegistry>,
    ) -> Self {
        Self {
            sync: model::directory_sync::DirectorySync::new(uuid::Uuid::new_v4().to_string()),
            agents: agents.into(),
            workspaces: workspaces.into(),
            projects: projects.into(),
        }
    }

    /// Use the host's shared directory generation and collection checkpoints.
    #[must_use]
    pub fn with_directory_sync(mut self, sync: model::directory_sync::DirectorySync) -> Self {
        self.sync = sync;
        self
    }

    /// List active Agent directory rows.
    ///
    /// # Errors
    /// Returns invalid-query or registry failures.
    pub fn list(
        &self,
        query: &AgentDirectoryQuery,
    ) -> Result<AgentDirectoryPage, AgentRuntimeError> {
        self.query(query)
    }

    /// List Agent history, including archived records when requested by the caller.
    ///
    /// # Errors
    /// Returns invalid-query or registry failures.
    pub fn history(
        &self,
        query: &AgentDirectoryQuery,
    ) -> Result<AgentDirectoryPage, AgentRuntimeError> {
        self.query(query)
    }

    /// Check whether an exact identity is occupied, including internal and archived Agents.
    /// This creation guard does not apply user-facing prefix or title resolution.
    /// # Errors
    /// Returns a registry error when the identity cannot be checked.
    pub fn contains_identity(&self, id: &str) -> Result<bool, AgentRuntimeError> {
        self.agents
            .get(id)
            .map(|agent| agent.is_some())
            .map_err(map_agent_registry)
    }

    /// Resolve a full ID, unique ID prefix, or exact full title.
    ///
    /// # Errors
    /// Returns an explicit missing/ambiguous result or registry failure.
    pub fn get(&self, identifier: &str) -> Result<ResolvedAgent, AgentRuntimeError> {
        let identifier = identifier.trim();
        if identifier.is_empty() {
            return Err(AgentRuntimeError::NotFound(String::new()));
        }
        let records = self.public_records()?;
        let record = if let Some(record) = records.iter().find(|record| record.id == identifier) {
            record.clone()
        } else {
            let prefix_matches = records
                .iter()
                .filter(|record| record.id.starts_with(identifier))
                .collect::<Vec<_>>();
            match prefix_matches.as_slice() {
                [record] => (*record).clone(),
                [] => Self::resolve_title(&records, identifier)?,
                _ => return Err(AgentRuntimeError::Ambiguous(identifier.to_owned())),
            }
        };
        let placement = self
            .placements()?
            .remove(&record.workspace_id.clone().unwrap_or_default());
        Ok(ResolvedAgent {
            agent: record,
            placement,
        })
    }

    /// Update title and/or labels on one stored Agent.
    ///
    /// # Errors
    /// Returns missing-Agent, invalid-request, or persistence failures.
    pub fn update(
        &self,
        agent_id: &str,
        name: Option<&str>,
        labels: Option<&BTreeMap<String, String>>,
        updated_at: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
        let title = name.map(str::trim).filter(|title| !title.is_empty());
        let labels = labels.filter(|labels| !labels.is_empty());
        if title.is_none() && labels.is_none() {
            return Err(AgentRuntimeError::InvalidRequest);
        }
        self.agents
            .update(agent_id, &|current| {
                let mut next = current.clone();
                if let Some(title) = title {
                    next.title = Some(title.to_owned());
                    next.title_origin = None;
                }
                if let Some(labels) = labels {
                    next.labels.extend(labels.clone());
                }
                updated_at.clone_into(&mut next.updated_at);
                next
            })
            .map_err(map_agent_registry)?
            .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.to_owned()))
    }

    /// Archive an Agent snapshot and eligible delegated children.
    ///
    /// # Errors
    /// Returns missing-Agent or persistence failures.
    pub fn archive(
        &self,
        agent_id: &str,
        archived_at: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
        archive::archive(self.agents.as_ref(), agent_id, archived_at)
    }

    /// Archive all Agents in the selected Workspace, retaining cross-Workspace descendants.
    /// # Errors
    /// Returns registry failures after any previously committed Agent updates.
    pub fn archive_workspaces(
        &self,
        workspace_ids: &[String],
        archived_at: &str,
    ) -> Result<Vec<String>, AgentRuntimeError> {
        archive::archive_workspaces(self.agents.as_ref(), workspace_ids, archived_at)
    }

    /// Permanently remove one Agent snapshot.
    ///
    /// # Errors
    /// Returns missing-Agent or persistence failures.
    pub fn delete(&self, agent_id: &str) -> Result<(), AgentRuntimeError> {
        if !self.agents.remove(agent_id).map_err(map_agent_registry)? {
            return Err(AgentRuntimeError::NotFound(agent_id.to_owned()));
        }
        Ok(())
    }

    /// Clear attention state for every selected Agent.
    ///
    /// # Errors
    /// Returns missing-Agent or persistence failures. Earlier updates may already be durable.
    pub fn clear_attention(
        &self,
        agent_ids: &[String],
        updated_at: &str,
    ) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeError> {
        agent_ids
            .iter()
            .map(|agent_id| {
                self.agents
                    .update(agent_id, &|current| {
                        let mut next = current.clone();
                        next.requires_attention = false;
                        next.attention_reason = None;
                        next.attention_timestamp = None;
                        updated_at.clone_into(&mut next.updated_at);
                        next
                    })
                    .map_err(map_agent_registry)?
                    .ok_or_else(|| AgentRuntimeError::NotFound(agent_id.clone()))
            })
            .collect()
    }

    /// Remove delegation and connection-owned open-tab labels.
    ///
    /// # Errors
    /// Returns missing-Agent or persistence failures.
    pub fn detach(
        &self,
        agent_id: &str,
        updated_at: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
        archive::detach(self.agents.as_ref(), agent_id, updated_at)
    }

    fn query(&self, query: &AgentDirectoryQuery) -> Result<AgentDirectoryPage, AgentRuntimeError> {
        if !(1..=DEFAULT_PAGE_LIMIT).contains(&query.limit) {
            return Err(AgentRuntimeError::InvalidRequest);
        }
        Self::paginate(self.matching_entries(query)?, query)
    }

    pub(crate) fn matching_entries(
        &self,
        query: &AgentDirectoryQuery,
    ) -> Result<Vec<AgentDirectoryEntry>, AgentRuntimeError> {
        let placements = self.placements()?;
        let search = search::Query::new(query.search.as_deref().unwrap_or_default());
        Ok(self
            .public_records()?
            .into_iter()
            .filter(|record| query.include_archived || record.archived_at.is_none())
            .filter_map(|agent| {
                let placement = placements.get(agent.workspace_id.as_deref()?)?.clone();
                Some(AgentDirectoryEntry { agent, placement })
            })
            .filter(|entry| matches_query(entry, query, &search))
            .collect::<Vec<_>>())
    }

    pub(crate) fn paginate(
        entries: Vec<AgentDirectoryEntry>,
        query: &AgentDirectoryQuery,
    ) -> Result<AgentDirectoryPage, AgentRuntimeError> {
        let sort = if query.sort.is_empty() {
            vec![AgentSort {
                key: AgentSortKey::UpdatedAt,
                direction: SortDirection::Desc,
            }]
        } else {
            query.sort.clone()
        };
        let entries = entries
            .into_iter()
            .map(|entry| Entry {
                id: entry.agent.id.clone(),
                values: sort
                    .iter()
                    .map(|sort| {
                        (
                            sort_key(sort.key).to_owned(),
                            sort_value(&entry.agent, sort.key),
                        )
                    })
                    .collect(),
                value: entry,
            })
            .collect();
        let sort = sort
            .iter()
            .map(|sort| Sort {
                key: sort_key(sort.key).to_owned(),
                direction: match sort.direction {
                    SortDirection::Asc => Direction::Asc,
                    SortDirection::Desc => Direction::Desc,
                },
            })
            .collect::<Vec<_>>();
        let page = pagination::paginate(entries, &sort, query.limit, query.cursor.as_deref())
            .map_err(|_| AgentRuntimeError::InvalidRequest)?;
        Ok(AgentDirectoryPage {
            entries: page.entries,
            next_cursor: page.next_cursor,
            prev_cursor: page.prev_cursor,
            has_more: page.has_more,
        })
    }

    fn public_records(&self) -> Result<Vec<PersistedAgentRuntimeRecord>, AgentRuntimeError> {
        Ok(self
            .agents
            .list()
            .map_err(map_agent_registry)?
            .into_iter()
            .filter(|record| !record.internal)
            .collect())
    }

    fn placements(&self) -> Result<BTreeMap<String, AgentPlacement>, AgentRuntimeError> {
        let projects = self
            .projects
            .list()
            .map_err(map_workspace_registry)?
            .into_iter()
            .map(|project| (project.project_id.clone(), project))
            .collect::<BTreeMap<_, _>>();
        Ok(self
            .workspaces
            .list()
            .map_err(map_workspace_registry)?
            .into_iter()
            .filter_map(|workspace| {
                let project = projects.get(&workspace.project_id)?;
                Some((
                    workspace.workspace_id.clone(),
                    placement(&workspace, project),
                ))
            })
            .collect())
    }

    fn resolve_title(
        records: &[PersistedAgentRuntimeRecord],
        title: &str,
    ) -> Result<PersistedAgentRuntimeRecord, AgentRuntimeError> {
        let matches = records
            .iter()
            .filter(|record| record.title.as_deref() == Some(title))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [record] => Ok((*record).clone()),
            [] => Err(AgentRuntimeError::NotFound(title.to_owned())),
            _ => Err(AgentRuntimeError::Ambiguous(title.to_owned())),
        }
    }
}

fn placement(
    workspace: &PersistedWorkspaceRecord,
    project: &PersistedProjectRecord,
) -> AgentPlacement {
    let is_git = project.kind == PersistedProjectKind::Git
        || workspace.kind != PersistedWorkspaceKind::Directory;
    AgentPlacement {
        active: workspace.archived_at.as_ref().is_none_or(String::is_empty)
            && project.archived_at.as_ref().is_none_or(String::is_empty),
        project_key: project
            .project_key
            .clone()
            .unwrap_or_else(|| project.project_id.clone()),
        project_name: project.display_name().to_owned(),
        workspace_name: workspace.display_name().to_owned(),
        cwd: workspace.cwd.clone(),
        is_git,
        current_branch: workspace.branch.clone(),
        worktree_root: is_git.then(|| {
            workspace
                .worktree_root
                .clone()
                .unwrap_or_else(|| workspace.cwd.clone())
        }),
        is_paseo_owned_worktree: workspace.is_paseo_owned_worktree,
        main_repo_root: workspace.main_repo_root.clone(),
    }
}

fn matches_query(
    entry: &AgentDirectoryEntry,
    query: &AgentDirectoryQuery,
    search: &search::Query,
) -> bool {
    let agent = &entry.agent;
    if query.active_scope && (agent.archived_at.is_some() || !entry.placement.active) {
        return false;
    }
    if !query
        .labels
        .iter()
        .all(|(key, value)| agent.labels.get(key) == Some(value))
    {
        return false;
    }
    if query
        .project_keys
        .as_ref()
        .is_some_and(|keys| !keys.contains(&entry.placement.project_key))
        || query
            .statuses
            .as_ref()
            .is_some_and(|statuses| !statuses.contains(&agent.last_status))
        || query
            .requires_attention
            .is_some_and(|required| required != agent.requires_attention)
    {
        return false;
    }
    let thinking = effective_thinking_option_id(agent);
    if query.thinking_option_id.as_ref().is_some_and(|required| {
        let required = match required {
            ThinkingOptionFilter::ProviderDefault => None,
            ThinkingOptionFilter::Selected(id) => Some(id.as_str()),
        };
        normalize_thinking_option_id(required) != thinking
    }) {
        return false;
    }
    search.matches([
        &entry.placement.workspace_name,
        agent.title.as_deref().unwrap_or_default(),
        entry
            .placement
            .current_branch
            .as_deref()
            .unwrap_or_default(),
        &entry.placement.project_name,
    ])
}

const fn sort_key(key: AgentSortKey) -> &'static str {
    match key {
        AgentSortKey::StatusPriority => "status_priority",
        AgentSortKey::CreatedAt => "created_at",
        AgentSortKey::UpdatedAt => "updated_at",
        AgentSortKey::Title => "title",
    }
}

fn sort_value(agent: &PersistedAgentRuntimeRecord, key: AgentSortKey) -> SortValue {
    match key {
        AgentSortKey::StatusPriority => SortValue::Number(i64::from(status_priority(agent))),
        AgentSortKey::CreatedAt => {
            parse_timestamp(&agent.created_at).map_or(SortValue::Null, SortValue::Number)
        }
        AgentSortKey::UpdatedAt => {
            parse_timestamp(resolved_updated_at(agent)).map_or(SortValue::Null, SortValue::Number)
        }
        AgentSortKey::Title => {
            SortValue::Text(agent.title.as_deref().unwrap_or_default().to_lowercase())
        }
    }
}

fn parse_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

/// Resolve the normalized runtime thinking selection, falling back to stored configuration.
#[must_use]
pub fn effective_thinking_option_id(record: &PersistedAgentRuntimeRecord) -> Option<&str> {
    normalize_thinking_option_id(
        record
            .runtime_info
            .as_ref()
            .and_then(|runtime| runtime.thinking_option_id.as_deref())
            .or_else(|| {
                record
                    .config
                    .as_ref()
                    .and_then(|config| config.thinking_option_id.as_deref())
            }),
    )
}

fn normalize_thinking_option_id(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn status_priority(record: &PersistedAgentRuntimeRecord) -> u8 {
    if matches!(
        record.attention_reason,
        Some(AgentAttentionReason::Permission)
    ) {
        return 0;
    }
    if record.last_status == AgentRuntimeStatus::Error
        || matches!(record.attention_reason, Some(AgentAttentionReason::Error))
    {
        return 1;
    }
    match record.last_status {
        AgentRuntimeStatus::Running => 2,
        AgentRuntimeStatus::Initializing => 3,
        AgentRuntimeStatus::Idle | AgentRuntimeStatus::Error | AgentRuntimeStatus::Closed => 4,
    }
}

const fn map_agent_registry(_error: AgentRuntimeRegistryError) -> AgentRuntimeError {
    AgentRuntimeError::AgentRegistry
}

const fn map_workspace_registry(_error: RegistryError) -> AgentRuntimeError {
    AgentRuntimeError::WorkspaceRegistry
}

#[cfg(test)]
mod tests;
