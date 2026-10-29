use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
mod bot;
mod catalog;
mod hub;
mod push;
mod schedule;
mod voice;

use api::{Api, LocalAddress, Services};
use browser::broker::Broker;
use chrono::{SecondsFormat, Utc};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use filesystem::files::service::files::Files;
use filesystem::forge::local::{forge::LocalForge, github_projects::LocalGithubProjects};
use filesystem::forge::service::Forge;
use filesystem::git::local::{
    checkout::LocalCheckout, provisioning::LocalDirectorySource, worktrees::LocalManagedWorktrees,
};
use filesystem::git::service::checkout::Checkout;
use filesystem::workspace_runtime::LocalWorkspaceRuntime;
use metadata::automation::local::LocalWorkspaceAutomation;
use metadata::automation::service::WorkspaceAutomation;
use metadata::daemon::service::{Daemon, DaemonRuntime};
use metadata::directory::service::directory::{Directory, DirectoryDependencies};
use metadata::directory::service::github_projects::GithubProjects;
use metadata::directory::service::workspace_names::WorkspaceNames;
use metadata::directory::service::workspace_state::WorkspaceState;
use metadata::labels::service::WorkspaceLabels;
use metadata::worktrees::service::workspace_recovery::WorkspaceRecovery;
use metadata::worktrees::service::worktrees::{WorkspaceWorktrees, Worktrees};
use model::LifecycleIntent;
use model::summary::SummaryGenerator;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use persistence::storage::summary_config::LocalSummaryConfiguration;
use persistence::storage::workspace_labels::FileWorkspaceLabelStore;
use provider::Providers;
use provider::service::agent_execution::{AgentExecution, ExecutionDependencies};
use provider::service::agent_manager::AgentManager;
use provider::service::agent_mcp::AgentMcp;
use provider::service::agent_runtime::AgentRuntimeDirectory;
use provider::service::agents::Agents;
use provider::service::workspace_attention::AgentWorkspaceAttention;
use provider::storage::SqliteCatalog;
use tokio::net::TcpListener;

use crate::config::Config;
use crate::instance::InstanceLease;

type HostLink = host_link::service::Service<
    crate::device::HttpCenter,
    crate::device::Store,
    crate::device::StatusRelay,
>;

pub(super) struct Server {
    listener: TcpListener,
    api: Api,
    // Own the directory lock until all accepted connections have stopped.
    instance: Arc<InstanceLease>,
    managed: HostLink,
    publication: hub::Worker,
}

impl Server {
    #[cfg(test)]
    pub(crate) async fn bind(config: Config) -> anyhow::Result<Self> {
        Self::bind_optional_diagnostics(config, None).await
    }

    /// Bind the daemon with the host's bounded evidence collector.
    /// Returns a ready server, or configuration, listener, storage or assembly errors.
    pub(crate) async fn bind_with_diagnostics(
        config: Config,
        diagnostics: Arc<dyn metadata::daemon::ports::DaemonDiagnostics>,
    ) -> anyhow::Result<Self> {
        Self::bind_optional_diagnostics(config, Some(diagnostics)).await
    }

    async fn bind_optional_diagnostics(
        config: Config,
        diagnostics: Option<Arc<dyn metadata::daemon::ports::DaemonDiagnostics>>,
    ) -> anyhow::Result<Self> {
        let data_dir = config.data_dir.clone();
        let listener = TcpListener::bind(config.listen)
            .await
            .context("bind server listener")?;
        let token = config.token.clone();
        let web_origins = config.web_origins.clone();
        let hub_notifications = config.hub_notifications;
        let address = listener
            .local_addr()
            .context("read server listener address")?;
        let mcp_token = secrecy::SecretString::from(uuid::Uuid::new_v4().to_string());
        let launch_mcp_token = mcp_token.clone();
        let (instance, services, sources, device) = tokio::task::spawn_blocking(move || {
            let instance = Arc::new(InstanceLease::acquire(&config.data_dir)?);
            let device = crate::device::load_state(&config.data_dir, instance.server_id)?;
            let (mut services, sources) =
                compose_services(&config, address, &instance, launch_mcp_token)?;
            if let Some(diagnostics) = diagnostics {
                services.metadata = services.metadata.map(|service| {
                    let mut dependencies = service.into_dependencies();
                    dependencies.daemon = dependencies.daemon.with_diagnostics(diagnostics);
                    metadata::Service::new(dependencies)
                });
            }
            Ok::<_, anyhow::Error>((instance, services, sources, device))
        })
        .await
        .context("join server initialization")??;
        let mut api = Api::new(
            address,
            instance.server_id.to_string(),
            instance.instance_id.to_string(),
            token,
            services,
        )?
        .with_agent_mcp_token(mcp_token)?
        .with_browser_origins(web_origins)?;
        let (store, state) = device;
        let relay = crate::device::StatusRelay::new(
            api.claim_managed_relay()?,
            &data_dir,
            instance.instance_id,
        );
        let managed = host_link::service::Service::new(
            crate::device::HttpCenter::new()?,
            store,
            relay,
            state,
            instance.instance_id,
        );
        api.set_publication_control(managed.handle())?;
        let publication = hub::start(
            &api,
            sources,
            &data_dir,
            instance.clone(),
            hub_notifications,
        )
        .await?;
        Ok(Self {
            listener,
            api,
            instance,
            managed,
            publication,
        })
    }

    pub(crate) fn address(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("bound TCP listener has a local address")
    }

    pub(crate) async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<Option<LifecycleIntent>> {
        let Self {
            listener,
            api,
            instance,
            managed,
            publication,
        } = self;
        let cancel = tokio_util::sync::CancellationToken::new();
        let manager_cancel = cancel.clone();
        let manager = tokio::spawn(managed.run(manager_cancel));
        let result: anyhow::Result<()> = async {
            let shutdown_api = api.clone();
            let shutdown_manager = cancel.clone();
            let server = axum::serve(
                listener,
                api.router()
                    .into_make_service_with_connect_info::<LocalAddress>(),
            )
            .with_graceful_shutdown(async move {
                tokio::select! {
                    () = shutdown => {},
                    () = shutdown_api.wait_draining() => {},
                }
                shutdown_manager.cancel();
                shutdown_api.begin_shutdown();
            })
            .into_future();
            tokio::pin!(server);
            // Readiness changes in the signal future before HTTP acceptance stops.
            // Also clean up WS tasks if the HTTP server terminates with an error.
            let result = tokio::select! {
                result = &mut server => Some(result),
                () = api.wait_draining() => None,
            };
            api.begin_shutdown();
            tokio::time::timeout(Duration::from_secs(15), async {
                let result = match result {
                    Some(result) => result,
                    None => server.await,
                };
                api.wait_closed().await;
                result.context("serve HTTP")
            })
            .await
            .context("server shutdown exceeded 15 seconds")??;
            Ok(())
        }
        .await;
        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(15), manager)
            .await
            .context("publication manager shutdown exceeded deadline")?
            .context("join publication manager")?;
        let lifecycle_intent = api.lifecycle_intent();
        publication.shutdown().await;
        // Drop routers and application storage before the data-directory instance lease.
        drop(api);
        drop(instance);
        result?;
        Ok(lifecycle_intent)
    }
}

fn compose_services(
    config: &Config,
    address: SocketAddr,
    instance: &Arc<InstanceLease>,
    mcp_token: secrecy::SecretString,
) -> anyhow::Result<(Services, hub::sources::Sources)> {
    let storage = open_directory_registries(config)?;
    let publication_sources = storage.publication_sources();
    let workspace_labels = compose_workspace_labels(config, &storage.workspaces)?;
    let skills = compose_skills(&config.data_dir)?;
    let agents = Agents::new(Box::new(catalog::OwnedCatalog {
        catalog: SqliteCatalog::open(&config.data_dir)?,
        _instance: instance.clone(),
    }));
    let server_id = instance.server_id.to_string();
    let providers = Providers::new(&config.data_dir);
    let MetadataServices {
        config: config_store,
        generator: summary_generator,
        names: workspace_names,
    } = compose_metadata(&config.data_dir, &storage.workspaces, &providers);
    let worktrees = Arc::new(Mutex::new(
        compose_worktrees(config, &storage.projects, &storage.workspaces, &server_id)
            .with_workspace_names(Arc::new(workspace_names.clone())),
    ));
    let WorkspaceServices {
        automation: workspace_automation,
        state: workspace_state,
        recovery: workspace_recovery,
    } = compose_workspace_services(
        config,
        &storage.workspaces,
        &storage.projects,
        &storage.agents,
    );
    let agent_mcp = AgentMcp::new(address, mcp_token, Arc::new(config_store.clone()));
    let daemon = compose_daemon(config_store, address, &server_id)?;
    let workspace_automation = Arc::new(Mutex::new(workspace_automation));
    let timeline = open_timeline(&config.data_dir)?;
    let terminals = compose_terminals(&storage.workspaces, &storage.projects, &storage.changes);
    let directory = compose_directory(
        config,
        &storage.projects,
        &storage.workspaces,
        server_id,
        storage.changes,
    )?
    .with_worktrees(Arc::new(WorkspaceWorktrees::new(worktrees.clone())))
    .with_workspace_names(workspace_names.clone())
    .with_activity_source(Arc::new(
        AgentWorkspaceAttention::new(Box::new(storage.agents.clone()))
            .with_timeline(timeline.clone()),
    ))
    .with_activity_source(Arc::new(terminals.activity_source()));
    let agent_execution = compose_provider(
        (storage.agents.clone(), timeline, agent_mcp),
        (&storage.workspaces, &storage.projects),
        instance,
        providers,
        (
            directory.clone(),
            summary_generator.clone(),
            workspace_names.clone(),
            workspace_automation.clone(),
        ),
    )?;
    let (schedules, bots) = compose_background(
        config,
        &agent_execution,
        (&storage.workspaces, &storage.projects),
        &directory,
        &worktrees,
    )?;
    let metadata = metadata::Service::new(metadata::Dependencies {
        workspace_names,
        push_tokens: compose_push(&config.data_dir, Arc::new(storage.agents))?,
        daemon,
        github_projects: GithubProjects::new(
            directory.clone(),
            Box::new(LocalGithubProjects::new()),
        ),
        directory,
        workspace_labels,
        workspace_automation,
        workspace_state,
        worktrees,
        workspace_recovery,
    });
    Ok((
        Services {
            bot: Some(bots),
            metadata: Some(metadata),
            filesystem: Some(compose_filesystem(config, &storage.workspaces)),
            provider: Some(provider::Service::new(provider::Dependencies {
                execution: agent_execution.clone(),
                summary_generator,
                agents,
                skills,
            })),
            schedule: Some(schedules),
            browser: Some(Broker::default()),
            voice: Some(voice::compose(agent_execution, &config.data_dir)?),
            terminal: Some(terminals),
        },
        publication_sources,
    ))
}

fn compose_workspace_labels(
    config: &Config,
    workspaces: &FileBackedWorkspaceRegistry,
) -> anyhow::Result<WorkspaceLabels> {
    Ok(WorkspaceLabels::new(Box::new(
        FileWorkspaceLabelStore::new(&config.data_dir, workspaces.clone()),
    ))?)
}

fn compose_background(
    config: &Config,
    execution: &AgentExecution,
    registries: (&FileBackedWorkspaceRegistry, &FileBackedProjectRegistry),
    directory: &Directory,
    worktrees: &Arc<Mutex<Worktrees>>,
) -> anyhow::Result<(::schedule::service::Schedules, ::bot::service::Bots)> {
    let bots = bot::compose(
        &config.data_dir,
        registries.0,
        registries.1,
        execution.clone(),
    )?;
    let schedules = schedule::compose(
        &config.data_dir,
        execution.clone(),
        directory.clone(),
        worktrees.clone(),
    )?;
    Ok((schedules, bots))
}

fn compose_filesystem(
    config: &Config,
    workspace_registry: &FileBackedWorkspaceRegistry,
) -> filesystem::Service {
    let (checkout, git_fetch) = compose_git(&config.data_dir, workspace_registry);
    filesystem::Service::new(filesystem::Dependencies {
        checkout,
        git_fetch,
        forge: Forge::new(Box::new(LocalForge::new())),
        files: compose_files(config),
    })
}

fn open_directory_registries(config: &Config) -> anyhow::Result<Registries> {
    let projects = FileBackedProjectRegistry::new(config.data_dir.join("projects/projects.json"));
    let workspaces =
        FileBackedWorkspaceRegistry::new(config.data_dir.join("projects/workspaces.json"));
    projects.initialize()?;
    workspaces.initialize()?;
    let changes = model::changes::Changes::default();
    let agents = FileBackedAgentRuntimeRegistry::new(config.data_dir.join("agents/agents.json"))
        .with_changes(changes.clone());
    agents.initialize()?;
    Ok(Registries {
        projects,
        workspaces,
        agents,
        changes,
    })
}

struct Registries {
    projects: FileBackedProjectRegistry,
    workspaces: FileBackedWorkspaceRegistry,
    agents: FileBackedAgentRuntimeRegistry,
    changes: model::changes::Changes,
}

impl Registries {
    fn publication_sources(&self) -> hub::sources::Sources {
        hub::sources::Sources {
            projects: Arc::new(self.projects.clone()),
            workspaces: Arc::new(self.workspaces.clone()),
            agents: Arc::new(self.agents.clone()),
            changes: self.changes.clone(),
        }
    }
}

fn compose_terminals(
    workspaces: &FileBackedWorkspaceRegistry,
    projects: &FileBackedProjectRegistry,
    changes: &model::changes::Changes,
) -> terminal::service::Terminals {
    let mut terminals = terminal::service::Terminals::new(
        Box::new(workspaces.clone()),
        Box::new(projects.clone()),
        Box::new(terminal::local::LocalRuntime),
    );
    terminals.set_directory_changes(changes.clone());
    terminals
}

fn compose_git(
    data_dir: &std::path::Path,
    workspace_registry: &FileBackedWorkspaceRegistry,
) -> (Checkout, filesystem::git::service::git_fetch::GitFetch) {
    let root = data_dir.join("worktrees");
    let checkout = Checkout::new(Box::new(LocalCheckout::new(root.clone())))
        .with_workspace_registry(Arc::new(workspace_registry.clone()));
    let fetch = filesystem::git::service::git_fetch::GitFetch::new(Arc::new(
        filesystem::git::local::git_fetch::LocalGitFetch::new(root),
    ));
    (checkout, fetch)
}

struct WorkspaceServices {
    automation: WorkspaceAutomation,
    state: WorkspaceState,
    recovery: WorkspaceRecovery,
}

fn compose_workspace_services(
    config: &Config,
    workspace_registry: &FileBackedWorkspaceRegistry,
    project_registry: &FileBackedProjectRegistry,
    agent_runtime_registry: &FileBackedAgentRuntimeRegistry,
) -> WorkspaceServices {
    let workspace_automation = WorkspaceAutomation::new(
        Box::new(workspace_registry.clone()),
        Box::new(LocalWorkspaceAutomation::new(Arc::new(
            LocalProjectConfigStore,
        ))),
    );
    let workspace_state = WorkspaceState::new(
        Box::new(AgentWorkspaceAttention::new(Box::new(
            agent_runtime_registry.clone(),
        ))),
        Box::new(workspace_registry.clone()),
    );
    let workspace_recovery = WorkspaceRecovery::new(
        Box::new(workspace_registry.clone()),
        Box::new(project_registry.clone()),
        Box::new(LocalManagedWorktrees::new(
            config.data_dir.join("worktrees"),
            Arc::new(LocalForge::new()),
        )),
    );
    WorkspaceServices {
        automation: workspace_automation,
        state: workspace_state,
        recovery: workspace_recovery,
    }
}

struct MetadataServices {
    config: FileDaemonConfigStore,
    generator: Arc<dyn SummaryGenerator>,
    names: WorkspaceNames,
}

fn compose_metadata(
    data_dir: &std::path::Path,
    registry: &FileBackedWorkspaceRegistry,
    providers: &Providers,
) -> MetadataServices {
    let config_store = FileDaemonConfigStore::with_defaults(data_dir.join("config.json"));
    let summary_generator = providers.summary_generator(Arc::new(LocalSummaryConfiguration::new(
        Arc::new(config_store.clone()),
    )));
    let workspace_names = WorkspaceNames::new(
        Arc::new(registry.clone()),
        api::summary_source(summary_generator.clone()),
        Arc::new(LocalCheckout::new(data_dir.join("worktrees"))),
    );
    MetadataServices {
        config: config_store,
        generator: summary_generator,
        names: workspace_names,
    }
}

fn compose_directory(
    config: &Config,
    project_registry: &FileBackedProjectRegistry,
    workspace_registry: &FileBackedWorkspaceRegistry,
    server_id: String,
    changes: model::changes::Changes,
) -> anyhow::Result<Directory> {
    let creations =
        persistence::storage::creation::open(config.data_dir.join("creations/receipts.json"))
            .map_err(|_| anyhow::anyhow!("initialize creation receipts"))?;
    Ok(Directory::new(DirectoryDependencies {
        projects: Box::new(project_registry.clone()),
        workspaces: Box::new(workspace_registry.clone()),
        source: Box::new(LocalDirectorySource),
        config_store: Box::new(LocalProjectConfigStore),
        icon_store: Box::new(LocalProjectIconStore::new(
            config.data_dir.join("projects/icons"),
        )),
        server_id,
    })
    .with_changes(changes.clone())
    .with_creations(creations)
    .with_runtime_source(Arc::new(
        LocalWorkspaceRuntime::new(
            LocalCheckout::new(config.data_dir.join("worktrees")),
            LocalForge::new(),
        )
        .with_changes(changes),
    )))
}

fn compose_worktrees(
    config: &Config,
    projects: &FileBackedProjectRegistry,
    workspaces: &FileBackedWorkspaceRegistry,
    server_id: &str,
) -> Worktrees {
    Worktrees::new(
        Box::new(projects.clone()),
        Box::new(workspaces.clone()),
        Box::new(LocalManagedWorktrees::new(
            config.data_dir.join("worktrees"),
            Arc::new(LocalForge::new()),
        )),
        server_id.to_owned(),
    )
}

fn compose_daemon(
    config_store: FileDaemonConfigStore,
    address: SocketAddr,
    server_id: &str,
) -> anyhow::Result<Daemon> {
    let daemon = Daemon::new(
        DaemonRuntime {
            server_id: server_id.to_owned(),
            version: Some(model::BUILD_VERSION.to_owned()),
            pid: std::process::id(),
            executable: std::env::current_exe()
                .context("resolve server executable")?
                .to_string_lossy()
                .into_owned(),
            started_at: Some(Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)),
            listen: address.to_string(),
        },
        Box::new(config_store),
    );
    daemon.get_config().context("initialize daemon config")?;
    Ok(daemon)
}

fn open_timeline(
    data_dir: &std::path::Path,
) -> anyhow::Result<provider::storage::timeline::Timeline> {
    provider::storage::timeline::Timeline::open(&data_dir.join("agents/timeline.sqlite3"))
        .map_err(|_| anyhow::anyhow!("initialize Agent timeline"))
}

fn compose_provider(
    agent_runtime: (
        FileBackedAgentRuntimeRegistry,
        provider::storage::timeline::Timeline,
        AgentMcp,
    ),
    registries: (&FileBackedWorkspaceRegistry, &FileBackedProjectRegistry),
    instance: &Arc<InstanceLease>,
    providers: Providers,
    metadata: (
        Directory,
        Arc<dyn SummaryGenerator>,
        WorkspaceNames,
        Arc<Mutex<WorkspaceAutomation>>,
    ),
) -> anyhow::Result<AgentExecution> {
    let (directory, generator, names, workspace_automation) = metadata;
    let (agent_runtime_registry, timeline, mcp) = agent_runtime;
    let (workspace_registry, project_registry) = registries;
    let mut manager = AgentManager::new(Box::new(agent_runtime_registry.clone()))
        .with_agent_mcp(mcp)
        .with_timeline(timeline)
        .with_creations(directory.creations())
        .with_summary_generation(generator)
        .with_workspace_names(Arc::new(names));
    providers.register(&mut manager)?;
    AgentExecution::spawn(ExecutionDependencies {
        manager,
        directory: AgentRuntimeDirectory::new(
            Box::new(agent_runtime_registry.clone()),
            Box::new(workspace_registry.clone()),
            Box::new(project_registry.clone()),
        )
        .with_directory_sync(directory.directory_sync()),
        registry: Box::new(agent_runtime_registry),
        workspaces: Box::new(workspace_registry.clone()),
        lifetime: instance.clone(),
        import_directory: Some(Arc::new(directory)),
        workspace_automation: Some(Arc::new(
            metadata::collaboration::SharedWorkspaceSetup::new(workspace_automation),
        )),
        projects: Box::new(project_registry.clone()),
    })
    .context("start Provider worker")
}

#[cfg(test)]
mod tests;

fn compose_push(
    data_dir: &std::path::Path,
    agents: Arc<dyn AgentRuntimeRegistry>,
) -> anyhow::Result<metadata::push::service::PushTokens> {
    let tokens = metadata::push::service::PushTokens::open(
        Box::new(persistence::storage::push::FileTokenStore::new(
            data_dir.join("push-tokens.json"),
        )),
        Utc::now().timestamp_millis(),
    )
    .map_err(|_| anyhow::anyhow!("initialize push token leases"))?;
    Ok(tokens.with_delivery(
        push::NativePush::from_environment(|name| std::env::var(name).ok())?,
        agents,
    ))
}

fn compose_skills(data: &std::path::Path) -> anyhow::Result<provider::skills::service::Skills> {
    let data = data
        .canonicalize()
        .context("resolve skills data directory")?;
    let home = std::env::var_os("AIT_SERVER_SKILLS_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(|| data.join("agent-home"), std::path::PathBuf::from);
    let source = std::env::var_os("AIT_SERVER_SKILLS_BUNDLE")
        .map_or_else(|| data.join("skills-bundle"), std::path::PathBuf::from);
    let targets = [".agents/skills", ".claude/skills", ".codex/skills"].map(|path| home.join(path));
    let store =
        provider::skills::local::LocalSkills::new(&source, &targets, &data.join("skills-state"))
            .map_err(|error| anyhow::anyhow!("invalid skills configuration: {error:?}"))?
            .with_bundled();
    let mut skills = provider::skills::service::Skills::new(Box::new(store));
    if let Err(error) = skills.distribute() {
        tracing::warn!(?error, "automatic Agent skill distribution failed");
    }
    Ok(skills)
}

fn compose_files(config: &Config) -> Files {
    Files::new(Box::new(filesystem::files::local::LocalFiles::new(
        std::env::var_os("HOME").map_or_else(|| config.data_dir.clone(), std::path::PathBuf::from),
        &config.data_dir,
    )))
}
