//! Transport composition of complete crate-level services into independently scoped request state.

use std::sync::{Arc, Mutex};

use filesystem::service::{
    checkout::Checkout, files::Files, forge::Forge, github_projects::GithubProjects,
    workspace_recovery::WorkspaceRecovery, worktrees::Worktrees,
};
use metadata::service::{
    daemon::Daemon, directory::Directory, workspace_automation::WorkspaceAutomation,
    workspace_labels::WorkspaceLabels, workspace_state::WorkspaceState,
};
use provider::service::{agent_execution::AgentExecution, agents::Agents};

use crate::Services;

/// Internal transport handles; only complete crate services can populate these fields.
#[derive(Default)]
pub(super) struct Parts {
    /// Bounded model-backed wording generation shared by title and Git use cases.
    pub(super) summary_source: Option<Arc<dyn model::summary::SummarySource>>,
    /// First-prompt workspace naming with independently drained background work.
    pub(super) workspace_names: Option<metadata::service::workspace_names::WorkspaceNames>,
    /// Persistent timed Agent executions.
    pub(super) schedules: Option<schedule::service::Schedules>,
    /// Connection-owned browser automation broker.
    pub(super) browser: Option<browser::broker::Broker>,
    /// Orchestration skill selection and installation.
    pub(super) skills: Option<filesystem::service::skills::Skills>,
    /// Durable push registration and lease renewal.
    pub(super) push_tokens: Option<metadata::service::push::PushTokens>,
    /// Connection-owned voice and dictation with independently selected speech engines.
    pub(super) speech: Option<voice::service::Speech>,
    /// Local PTY terminal lifecycle, input, capture, and streaming.
    pub(super) terminals: Option<terminal::service::Terminals>,
    /// Native Provider execution and coordinated Agent runtime metadata.
    pub(super) agent_execution: Option<AgentExecution>,
    /// Filesystem workspace recovery operations.
    pub(super) workspace_recovery: Option<WorkspaceRecovery>,
    /// Filesystem github projects operations.
    pub(super) github_projects: Option<GithubProjects>,
    /// Versioned Agent presets and explicit default selection.
    pub(super) agents: Option<Agents>,
    /// Git checkout status, diff, refresh, and history use cases.
    pub(super) checkout: Option<Checkout>,
    /// Background origin fetches for actively observed workspace repositories.
    pub(super) git_fetch: Option<filesystem::service::git_fetch::GitFetch>,
    /// Daemon status, mutable configuration, diagnostics, and update boundary.
    pub(super) daemon: Option<Daemon>,
    /// Project and workspace registries.
    pub(super) directory: Option<Directory>,
    /// Forge search and pull request use cases.
    pub(super) forge: Option<Forge>,
    /// Scoped filesystem, upload, and download operations.
    pub(super) files: Option<Files>,
    /// Workspace label catalog, assignment, and subscription use cases.
    pub(super) workspace_labels: Option<WorkspaceLabels>,
    /// Workspace setup and configured script runtime.
    pub(super) workspace_automation: Option<Arc<Mutex<WorkspaceAutomation>>>,
    /// Workspace attention and archived-placement recovery use cases.
    pub(super) workspace_state: Option<WorkspaceState>,
    /// Managed Git worktree lifecycle use cases.
    pub(super) worktrees: Option<Arc<Mutex<Worktrees>>>,
}

impl From<Services> for Parts {
    fn from(services: Services) -> Self {
        let mut parts = Self {
            schedules: services.schedule,
            browser: services.browser,
            speech: services.voice,
            terminals: services.terminal,
            ..Self::default()
        };
        if let Some(metadata) = services.metadata {
            let metadata = metadata.into_dependencies();
            parts.workspace_names = Some(metadata.workspace_names);
            parts.push_tokens = Some(metadata.push_tokens);
            parts.daemon = Some(metadata.daemon);
            parts.directory = Some(metadata.directory);
            parts.workspace_labels = Some(metadata.workspace_labels);
            parts.workspace_automation = Some(metadata.workspace_automation);
            parts.workspace_state = Some(metadata.workspace_state);
        }
        if let Some(filesystem) = services.filesystem {
            let filesystem = filesystem.into_dependencies();
            parts.skills = Some(filesystem.skills);
            parts.workspace_recovery = Some(filesystem.workspace_recovery);
            parts.github_projects = Some(filesystem.github_projects);
            parts.checkout = Some(filesystem.checkout);
            parts.git_fetch = Some(filesystem.git_fetch);
            parts.forge = Some(filesystem.forge);
            parts.files = Some(filesystem.files);
            parts.worktrees = Some(filesystem.worktrees);
        }
        if let Some(provider) = services.provider {
            let provider = provider.into_dependencies();
            parts.summary_source = Some(summary_source(provider.summary_generator));
            parts.agent_execution = Some(provider.execution);
            parts.agents = Some(provider.agents);
        }
        parts
    }
}

/// Adapt a provider-owned generator to the Workspace and Git summary consumption port.
///
/// `generator` is the shared provider instance. Returns a source that forwards requests and
/// shutdown directly, without adding a queue, worker, cache, or independent lifetime.
#[must_use]
pub fn summary_source(
    generator: Arc<dyn provider::summary::SummaryGenerator>,
) -> Arc<dyn model::summary::SummarySource> {
    Arc::new(SummarySource(generator))
}

/// Adapt installed `automation` to setup requests sharing its existing lock and task owner.
/// Returns none when automation is absent; no independent runtime or lock is created.
pub(super) fn workspace_setup(
    automation: Option<&Arc<Mutex<WorkspaceAutomation>>>,
) -> Option<Arc<dyn model::workspace::lifecycle::WorkspaceSetup>> {
    automation.map(|automation| {
        Arc::new(
            metadata::service::workspace_collaboration::SharedWorkspaceSetup::new(
                automation.clone(),
            ),
        ) as Arc<dyn model::workspace::lifecycle::WorkspaceSetup>
    })
}

#[derive(Debug)]
struct SummarySource(Arc<dyn provider::summary::SummaryGenerator>);

impl model::summary::SummarySource for SummarySource {
    fn generate(
        &self,
        request: model::summary::SummaryRequest,
    ) -> model::summary::SummaryFuture<'_> {
        self.0.generate(request)
    }
    fn shutdown(&self) {
        self.0.shutdown();
    }
}

#[cfg(test)]
mod tests;
