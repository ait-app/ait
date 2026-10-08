use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
mod catalog;
mod schedule;
mod summary;
mod voice;

use api::{Api, LocalAddress, Services};
use browser::broker::Broker;
use chrono::{SecondsFormat, Utc};
use domain::agent_runtime::registry::AgentRuntimeRegistry;
use filesystem::files::service::files::Files;
use filesystem::forge::local::{forge::LocalForge, github_projects::LocalGithubProjects};
use filesystem::forge::service::forge::Forge;
use filesystem::forge::service::github_projects::GithubProjects;
use filesystem::git::local::{checkout::LocalCheckout, provisioning::LocalDirectorySource};
use filesystem::git::service::checkout::Checkout;
use filesystem::workspace_runtime::LocalWorkspaceRuntime;
use filesystem::worktrees::local::worktrees::LocalManagedWorktrees;
use filesystem::worktrees::service::workspace_recovery::WorkspaceRecovery;
use filesystem::worktrees::service::worktrees::{WorkspaceWorktrees, Worktrees};
use metadata::local::workspace_automation::LocalWorkspaceAutomation;
use metadata::service::daemon::{Daemon, DaemonRuntime};
use metadata::service::directory::{Directory, DirectoryDependencies};
use metadata::service::workspace_automation::WorkspaceAutomation;
use metadata::service::workspace_labels::WorkspaceLabels;
use metadata::service::workspace_names::WorkspaceNames;
use metadata::service::workspace_state::WorkspaceState;
use model::LifecycleIntent;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::daemon_config::FileDaemonConfigStore;
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use persistence::storage::workspace_labels::FileWorkspaceLabelStore;
use provider::Providers;
use provider::service::agent_execution::{AgentExecution, ExecutionDependencies};
use provider::service::agent_manager::AgentManager;
use provider::service::agent_runtime::AgentRuntimeDirectory;
use provider::service::agents::Agents;
use provider::service::workspace_attention::AgentWorkspaceAttention;
use provider::storage::SqliteCatalog;
use provider::summary::SummaryGenerator;
use tokio::net::TcpListener;

use crate::config::Config;
use crate::instance::InstanceLease;

pub(super) struct Server {
    listener: TcpListener,
    api: Api,
    // Own the directory lock until all accepted connections have stopped.
    instance: Arc<InstanceLease>,
}

impl Server {
    #[cfg(test)]
    pub async fn bind(config: Config) -> anyhow::Result<Self> {
        Self::bind_optional_diagnostics(config, None).await
    }

    /// Bind the daemon with the host's bounded evidence collector.
    /// Returns a ready server, or configuration, listener, storage or assembly errors.
    pub async fn bind_with_diagnostics(
        config: Config,
        diagnostics: Arc<dyn metadata::ports::diagnostics::DaemonDiagnostics>,
    ) -> anyhow::Result<Self> {
        Self::bind_optional_diagnostics(config, Some(diagnostics)).await
    }

    async fn bind_optional_diagnostics(
        config: Config,
        diagnostics: Option<Arc<dyn metadata::ports::diagnostics::DaemonDiagnostics>>,
    ) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(config.listen)
            .await
            .context("bind server listener")?;
        let token = config.token.clone();
        let web_origins = config.web_origins.clone();
        let address = listener
            .local_addr()
            .context("read server listener address")?;
        let (instance, services) = tokio::task::spawn_blocking(move || {
            let instance = Arc::new(InstanceLease::acquire(&config.data_dir)?);
            let mut services = compose_services(&config, address, &instance)?;
            if let Some(diagnostics) = diagnostics {
                services.metadata = services.metadata.map(|service| {
                    let mut dependencies = service.into_dependencies();
                    dependencies.daemon = dependencies.daemon.with_diagnostics(diagnostics);
                    metadata::Service::new(dependencies)
                });
            }
            Ok::<_, anyhow::Error>((instance, services))
        })
        .await
        .context("join server initialization")??;
        let api = Api::new(
            address,
            instance.server_id.to_string(),
            instance.instance_id.to_string(),
            token,
            services,
        )?
        .with_browser_origins(web_origins)?;
        Ok(Self {
            listener,
            api,
            instance,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("bound TCP listener has a local address")
    }

    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<Option<LifecycleIntent>> {
        let Self {
            listener,
            api,
            instance,
        } = self;
        let result: anyhow::Result<()> = async {
            let shutdown_api = api.clone();
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
        let lifecycle_intent = api.lifecycle_intent();
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
) -> anyhow::Result<Services> {
    let (project_registry, workspace_registry) = open_directory_registries(config)?;
    let changes = model::changes::Changes::default();
    let agent_runtime_registry =
        FileBackedAgentRuntimeRegistry::new(config.data_dir.join("agents/agents.json"))
            .with_changes(changes.clone());
    agent_runtime_registry.initialize()?;
    let workspace_labels = WorkspaceLabels::new(Box::new(FileWorkspaceLabelStore::new(
        &config.data_dir,
        workspace_registry.clone(),
    )))?;
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
    } = compose_metadata(&config.data_dir, &workspace_registry, &providers);
    let worktrees = Arc::new(Mutex::new(
        compose_worktrees(config, &project_registry, &workspace_registry, &server_id)
            .with_workspace_names(Arc::new(workspace_names.clone())),
    ));
    let WorkspaceServices {
        automation: workspace_automation,
        state: workspace_state,
        recovery: workspace_recovery,
    } = compose_workspace_services(
        config,
        &workspace_registry,
        &project_registry,
        &agent_runtime_registry,
    );
    let daemon = compose_daemon(config_store, address, &server_id)?;
    let workspace_automation = Arc::new(Mutex::new(workspace_automation));
    let timeline = open_timeline(&config.data_dir)?;
    let terminals = compose_terminals(&workspace_registry, &project_registry, &changes);
    let directory = compose_directory(
        config,
        &project_registry,
        &workspace_registry,
        server_id,
        changes,
    )?
    .with_worktrees(Arc::new(WorkspaceWorktrees::new(worktrees.clone())))
    .with_workspace_names(workspace_names.clone())
    .with_activity_source(Arc::new(
        AgentWorkspaceAttention::new(Box::new(agent_runtime_registry.clone()))
            .with_timeline(timeline.clone()),
    ))
    .with_activity_source(Arc::new(terminals.activity_source()));
    let agent_execution = compose_provider(
        (agent_runtime_registry, timeline),
        (&workspace_registry, &project_registry),
        instance,
        providers,
        (
            directory.clone(),
            summary_generator.clone(),
            workspace_names.clone(),
            workspace_automation.clone(),
        ),
    )?;
    let schedules = schedule::compose(
        &config.data_dir,
        agent_execution.clone(),
        directory.clone(),
        worktrees.clone(),
    )?;
    let metadata = metadata::Service::new(metadata::Dependencies {
        workspace_names,
        push_tokens: compose_push(&config.data_dir)?,
        daemon,
        directory: directory.clone(),
        workspace_labels,
        workspace_automation,
        workspace_state,
    });
    let filesystem = compose_filesystem(
        config,
        &workspace_registry,
        directory,
        worktrees,
        workspace_recovery,
    )?;
    Ok(Services {
        metadata: Some(metadata),
        filesystem: Some(filesystem),
        provider: Some(provider::Service::new(provider::Dependencies {
            summary_generator,
            execution: agent_execution.clone(),
            agents,
        })),
        schedule: Some(schedules),
        browser: Some(Broker::default()),
        voice: Some(voice::compose(agent_execution, &config.data_dir)?),
        terminal: Some(terminals),
    })
}

fn compose_filesystem(
    config: &Config,
    workspace_registry: &FileBackedWorkspaceRegistry,
    directory: Directory,
    worktrees: Arc<Mutex<Worktrees>>,
    workspace_recovery: WorkspaceRecovery,
) -> anyhow::Result<filesystem::Service> {
    let (checkout, git_fetch) = compose_git(&config.data_dir, workspace_registry);
    Ok(filesystem::Service::new(filesystem::Dependencies {
        skills: compose_skills(&config.data_dir)?,
        workspace_recovery,
        github_projects: GithubProjects::new(
            Arc::new(directory),
            Box::new(LocalGithubProjects::new()),
        ),
        checkout,
        git_fetch,
        forge: Forge::new(Box::new(LocalForge::new())),
        files: compose_files(config),
        worktrees,
    }))
}

fn open_directory_registries(
    config: &Config,
) -> anyhow::Result<(FileBackedProjectRegistry, FileBackedWorkspaceRegistry)> {
    let projects = FileBackedProjectRegistry::new(config.data_dir.join("projects/projects.json"));
    let workspaces =
        FileBackedWorkspaceRegistry::new(config.data_dir.join("projects/workspaces.json"));
    projects.initialize()?;
    workspaces.initialize()?;
    Ok((projects, workspaces))
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
    let summary_generator = providers.summary_generator(Arc::new(summary::Configuration(
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
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
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
    agent_storage: (
        FileBackedAgentRuntimeRegistry,
        provider::storage::timeline::Timeline,
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
    let (agent_runtime_registry, timeline) = agent_storage;
    let (workspace_registry, project_registry) = registries;
    let mut manager = AgentManager::new(Box::new(agent_runtime_registry.clone()))
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
            metadata::service::workspace_collaboration::SharedWorkspaceSetup::new(
                workspace_automation,
            ),
        )),
        projects: Box::new(project_registry.clone()),
    })
    .context("start Provider worker")
}

#[cfg(test)]
mod tests;

fn compose_push(data_dir: &std::path::Path) -> anyhow::Result<metadata::service::push::PushTokens> {
    metadata::service::push::PushTokens::open(
        Box::new(persistence::storage::push::FileTokenStore::new(
            data_dir.join("push-tokens.json"),
        )),
        Utc::now().timestamp_millis(),
    )
    .map_err(|_| anyhow::anyhow!("initialize push token leases"))
}

fn compose_skills(
    data: &std::path::Path,
) -> anyhow::Result<filesystem::skills::service::skills::Skills> {
    let data = data
        .canonicalize()
        .context("resolve skills data directory")?;
    let home = std::env::var_os("AIT_SERVER_SKILLS_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map_or_else(|| data.join("agent-home"), std::path::PathBuf::from);
    let source = std::env::var_os("AIT_SERVER_SKILLS_BUNDLE")
        .map_or_else(|| data.join("skills-bundle"), std::path::PathBuf::from);
    let targets = [".agents/skills", ".claude/skills", ".codex/skills"].map(|path| home.join(path));
    let store = filesystem::skills::local::skills::LocalSkills::new(
        &source,
        &targets,
        &data.join("skills-state"),
    )
    .map_err(|error| anyhow::anyhow!("invalid skills configuration: {error:?}"))?;
    Ok(filesystem::skills::service::skills::Skills::new(Box::new(
        store,
    )))
}

fn compose_files(config: &Config) -> Files {
    Files::new(Box::new(filesystem::files::local::files::LocalFiles::new(
        std::env::var_os("HOME").map_or_else(|| config.data_dir.clone(), std::path::PathBuf::from),
        &config.data_dir,
    )))
}
