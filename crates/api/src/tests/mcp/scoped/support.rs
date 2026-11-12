//! Served API fixture for Bot and subtree credentials on the Agent MCP route.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use ::bot::mcp::Scopes;
use ::bot::ports::{Credentials, Executor};
use ::bot::service::{Bots, Dependencies};
use domain::ConversationId;
use domain::bot::{BOT_SCOPE_LABEL, BotState, SessionOwner};
use domain::schedule::{Schedule, Target};
use filesystem::git::local::provisioning::LocalDirectorySource;
use metadata::directory::service::directory::{Directory, DirectoryDependencies};
use model::bot::Store;
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::bot::FileStore;
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::ports::mcp_scope::{McpAccess, McpScopes, McpSubject};
use provider::service::agent_execution::ExecutionDependencies;
use provider::service::agent_manager::AgentManager;
use provider::service::agent_runtime::AgentRuntimeDirectory;
use provider::service::agents::Agents;
use provider::service::bot_execution::Action;
use provider::skills::local::LocalSkills;
use provider::skills::service::Skills;
use schedule::ports::{Outcome, Progress, Runner};
use tokio_util::sync::CancellationToken;

use super::*;
use crate::connection::rpc;

/// Hidden native session identity the simulated Bot conversation was opened with.
pub(super) const BOT_SESSION: &str = "bot-hidden-session";

/// Schedules never run in these tests, and Bot conversations only once a test lets them.
#[derive(Debug)]
struct Idle;

/// Bot native sessions: without [`Turns::run`] every turn fails to open; with it, turns stay
/// running until [`Turns::release`] and then complete with a reply.
#[derive(Debug, Default)]
pub(super) struct Turns {
    run: AtomicBool,
    released: AtomicBool,
}

impl Turns {
    /// Let Bot turns open and run, holding each one running until [`Turns::release`].
    pub(super) fn run(&self) {
        self.run.store(true, Ordering::SeqCst);
    }

    /// Let every running and later Bot turn complete.
    pub(super) fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl Executor for Turns {
    async fn execute(&self, _: SessionOwner, action: Action) -> Result<Value, model::ErrorCode> {
        if !self.run.load(Ordering::SeqCst) {
            return Ok(json!({"status":"idle","lastMessage":null}));
        }
        Ok(match action {
            Action::Create { .. } => json!({"created":true,"instructionDelivery":"system_prompt"}),
            Action::Status { .. } | Action::Poll { .. } if self.released.load(Ordering::SeqCst) => {
                json!({"status":"idle","lastMessage":"reported"})
            }
            Action::Status { .. } | Action::Poll { .. } => json!({"status":"running"}),
            Action::Send { .. }
            | Action::Release { .. }
            | Action::Close { .. }
            | Action::Cancel { .. }
            | Action::Permission { .. } => json!({}),
        })
    }
}

impl Credentials for Idle {
    fn resolve(&self, _: &str) -> Result<SecretString, model::ErrorCode> {
        Err(model::ErrorCode::BotIo)
    }
}

#[async_trait::async_trait]
impl Runner for Idle {
    async fn resolve(
        &self,
        _: String,
        target: Target,
    ) -> Result<Target, domain::storage::schedule::Error> {
        Ok(target)
    }

    async fn run(&self, _: Schedule, _: String, _: Progress, _: CancellationToken) -> Outcome {
        Outcome {
            agent_id: None,
            workspace_id: None,
            output: None,
            error: Some("idle".to_owned()),
            target_gone: false,
        }
    }
}

#[derive(Debug)]
struct Silent;

#[async_trait::async_trait]
impl model::summary::SummaryGenerator for Silent {
    async fn generate(
        &self,
        _: domain::summary::SummaryRequest,
    ) -> Result<Value, domain::summary::SummaryError> {
        Err(domain::summary::SummaryError::Unavailable)
    }

    fn shutdown(&self) {}
}

/// Terminal processes that stay alive without an operating-system PTY.
#[derive(Debug)]
struct Pty;

impl terminal::ports::Runtime for Pty {
    fn directory(&self, path: &str) -> Result<String, terminal::Error> {
        std::fs::canonicalize(path)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|_| terminal::Error::Invalid)
    }

    fn spawn(
        &self,
        _: &terminal::ports::Launch,
    ) -> Result<Box<dyn terminal::ports::Process>, terminal::Error> {
        Ok(Box::new(Shell::default()))
    }
}

#[derive(Debug, Default)]
struct Shell {
    killed: bool,
}

impl terminal::ports::Process for Shell {
    fn title(&self) -> Option<String> {
        None
    }

    fn exited(&mut self) -> Result<bool, terminal::Error> {
        Ok(self.killed)
    }

    fn send(&mut self, _: &terminal::protocol::Input) -> Result<(), terminal::Error> {
        Ok(())
    }

    fn observe(
        &mut self,
        _: Option<u64>,
        _: Option<&terminal::protocol::Restore>,
    ) -> Result<terminal::ports::Observation, terminal::Error> {
        Err(terminal::Error::NotFound)
    }

    fn capture(&self) -> Result<Vec<String>, terminal::Error> {
        Ok(vec!["prompt".to_owned()])
    }

    fn kill(&mut self) -> Result<(), terminal::Error> {
        self.killed = true;
        Ok(())
    }
}

/// A served API with Bot, schedule, terminal, metadata and offline Codex provider services.
///
/// The Bot's root Workspace is a Git checkout in its own project; `other` is an unrelated
/// Workspace in a second project that no scoped credential may reach.
pub(super) struct Scoped {
    pub(super) fixture: Fixture,
    pub(super) scopes: Arc<Scopes>,
    /// The served Bot service, for feeding notifications as the daemon does.
    pub(super) bots: Bots,
    /// The Bot service's native sessions.
    pub(super) turns: Arc<Turns>,
    store: Arc<FileStore>,
    pub(super) workspace: String,
    pub(super) cwd: PathBuf,
    pub(super) other: String,
    pub(super) other_cwd: PathBuf,
    _root: tempfile::TempDir,
}

struct Registries {
    projects: FileBackedProjectRegistry,
    workspaces: FileBackedWorkspaceRegistry,
    agents: FileBackedAgentRuntimeRegistry,
}

/// Initialize `directory` as a Git checkout with one commit, so worktrees can branch off it.
fn git_checkout(directory: &std::path::Path) {
    for arguments in [
        &["init", "--quiet", "--initial-branch=main"][..],
        &[
            "-c",
            "user.name=Ait",
            "-c",
            "user.email=ait@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "--message=initial",
        ],
    ] {
        let status = std::process::Command::new("git")
            .args(arguments)
            .current_dir(directory)
            .status()
            .unwrap();
        assert!(status.success(), "git {arguments:?}");
    }
}

fn metadata(
    data: &std::path::Path,
    registries: &Registries,
    directory: Directory,
) -> metadata::Service {
    use filesystem::forge::local::{forge::LocalForge, github_projects::LocalGithubProjects};
    use filesystem::git::local::{checkout::LocalCheckout, worktrees::LocalManagedWorktrees};
    use metadata::automation::local::LocalWorkspaceAutomation;
    use metadata::automation::service::WorkspaceAutomation;
    use metadata::daemon::service::{Daemon, DaemonRuntime};
    use metadata::directory::service::github_projects::GithubProjects;
    use metadata::directory::service::workspace_names::WorkspaceNames;
    use metadata::directory::service::workspace_state::WorkspaceState;
    use metadata::labels::service::WorkspaceLabels;
    use metadata::worktrees::service::workspace_recovery::WorkspaceRecovery;
    use metadata::worktrees::service::worktrees::Worktrees;
    use persistence::storage::workspace_labels::FileWorkspaceLabelStore;
    use provider::service::workspace_attention::AgentWorkspaceAttention;

    let managed = data.join("worktrees");
    let worktrees = || {
        Box::new(LocalManagedWorktrees::new(
            managed.clone(),
            Arc::new(LocalForge::new()),
        ))
    };
    metadata::Service::new(metadata::Dependencies {
        push_tokens: metadata::push::service::PushTokens::open(
            Box::new(persistence::storage::push::FileTokenStore::new(
                data.join("push-tokens.json"),
            )),
            0,
        )
        .unwrap(),
        daemon: Daemon::new(
            DaemonRuntime {
                server_id: "test-server".to_owned(),
                version: None,
                pid: std::process::id(),
                executable: "ait-server".to_owned(),
                started_at: None,
                listen: "127.0.0.1:7316".to_owned(),
            },
            Box::new(
                persistence::storage::daemon_config::FileDaemonConfigStore::with_defaults(
                    data.join("config.json"),
                ),
            ),
        ),
        github_projects: GithubProjects::new(
            directory.clone(),
            Box::new(LocalGithubProjects::new()),
        ),
        directory,
        workspace_labels: WorkspaceLabels::new(Box::new(FileWorkspaceLabelStore::new(
            data,
            registries.workspaces.clone(),
        )))
        .unwrap(),
        workspace_automation: Arc::new(Mutex::new(WorkspaceAutomation::new(
            Box::new(registries.workspaces.clone()),
            Box::new(LocalWorkspaceAutomation::new(Arc::new(
                LocalProjectConfigStore,
            ))),
        ))),
        workspace_state: WorkspaceState::new(
            Box::new(AgentWorkspaceAttention::new(Box::new(
                registries.agents.clone(),
            ))),
            Box::new(registries.workspaces.clone()),
        ),
        workspace_names: WorkspaceNames::new(
            Arc::new(registries.workspaces.clone()),
            summary_source(Arc::new(Silent)),
            Arc::new(LocalCheckout::new(managed.clone())),
        ),
        worktrees: Arc::new(Mutex::new(Worktrees::new(
            Box::new(registries.projects.clone()),
            Box::new(registries.workspaces.clone()),
            worktrees(),
            "test-server".to_owned(),
        ))),
        workspace_recovery: WorkspaceRecovery::new(
            Box::new(registries.workspaces.clone()),
            Box::new(registries.projects.clone()),
            worktrees(),
        ),
    })
}

fn provider(base: &std::path::Path, registries: &Registries) -> provider::Service {
    let data = base.join("data");
    let codex = base.join("codex");
    symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../provider/codex/tests/fixtures/codex_app_server.py"),
        &codex,
    )
    .unwrap();
    let mut manager = AgentManager::new(Box::new(registries.agents.clone())).with_timeline(
        provider::storage::timeline::Timeline::open(&data.join("timeline.sqlite3")).unwrap(),
    );
    provider_builtin::configured(&data, |name| {
        (name == "AIT_SERVER_CODEX_BIN").then(|| codex.clone().into_os_string())
    })
    .register(&mut manager)
    .unwrap();
    let execution = AgentExecution::spawn(ExecutionDependencies {
        manager,
        directory: AgentRuntimeDirectory::new(
            Box::new(registries.agents.clone()),
            Box::new(registries.workspaces.clone()),
            Box::new(registries.projects.clone()),
        ),
        registry: Box::new(registries.agents.clone()),
        workspaces: Box::new(registries.workspaces.clone()),
        projects: Box::new(registries.projects.clone()),
        import_directory: None,
        workspace_automation: None,
        lifetime: Arc::new(()),
    })
    .unwrap();
    let skills = base.join("skills");
    let targets = ["a", "b", "c"].map(|name| skills.join(name));
    provider::Service::new(provider::Dependencies {
        agents: Agents::new(Box::new(
            provider::storage::SqliteCatalog::open(&data).unwrap(),
        )),
        skills: Skills::new(Box::new(
            LocalSkills::new(&skills.join("source"), &targets, &skills.join("state")).unwrap(),
        )),
        execution,
        summary_generator: Arc::new(Silent),
    })
}

impl Scoped {
    pub(super) async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Canonical paths keep macOS temporary directories under `/var` consistent.
        let base = root.path().canonicalize().unwrap();
        let data = base.join("data");
        let cwd = base.join("work");
        let other_cwd = base.join("elsewhere");
        for directory in [&data, &cwd, &other_cwd] {
            std::fs::create_dir_all(directory).unwrap();
        }
        git_checkout(&cwd);
        let registries = Registries {
            projects: FileBackedProjectRegistry::new(data.join("projects.json")),
            workspaces: FileBackedWorkspaceRegistry::new(data.join("workspaces.json")),
            agents: FileBackedAgentRuntimeRegistry::new(data.join("agents.json")),
        };
        let directory = Directory::new(DirectoryDependencies {
            projects: Box::new(registries.projects.clone()),
            workspaces: Box::new(registries.workspaces.clone()),
            source: Box::new(LocalDirectorySource),
            config_store: Box::new(LocalProjectConfigStore),
            icon_store: Box::new(LocalProjectIconStore::new(data.join("icons"))),
            server_id: "test-server".into(),
        });
        let open = |path: &PathBuf| {
            directory
                .open_workspace(path.to_str().unwrap(), "2026-11-06T00:00:00Z")
                .unwrap()
                .workspace_id
        };
        let (workspace, other) = (open(&cwd), open(&other_cwd));
        let scopes = Scopes::new();
        let store = Arc::new(FileStore::new(data.join("bots")));
        let turns = Arc::new(Turns::default());
        let bots = Bots::spawn(Dependencies {
            store: store.clone(),
            workspaces: Arc::new(registries.workspaces.clone()),
            projects: Arc::new(registries.projects.clone()),
            executor: turns.clone(),
            credentials: Arc::new(Idle),
            scopes: scopes.clone(),
        })
        .unwrap();
        let schedules = Schedules::spawn(
            Box::new(persistence::storage::schedule::FileStore::new(
                data.join("schedules.json"),
            )),
            Arc::new(Idle),
        )
        .unwrap();
        let terminals = Terminals::new(
            Box::new(registries.workspaces.clone()),
            Box::new(registries.projects.clone()),
            Box::new(Pty),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Api::new(
            listener.local_addr().unwrap(),
            "stable".to_owned(),
            "instance".to_owned(),
            TOKEN.into(),
            Services {
                bot: Some(bots.clone()),
                filesystem: Some(chunks::filesystem_service::service(&data)),
                metadata: Some(metadata(&data, &registries, directory)),
                provider: Some(provider(&base, &registries)),
                schedule: Some(schedules),
                terminal: Some(terminals),
                ..Services::default()
            },
        )
        .unwrap()
        .with_agent_mcp_token(AGENT_MCP_TOKEN.into())
        .unwrap();
        Self {
            fixture: Fixture::serve(listener, api),
            scopes,
            bots,
            turns,
            store,
            workspace,
            cwd,
            other,
            other_cwd,
            _root: root,
        }
    }

    pub(super) async fn rpc(&self, method: &str, params: Value) -> Value {
        rpc::execute(&self.fixture.api.shared, method, params)
            .await
            .unwrap_or_else(|error| panic!("{method}: {error:?}"))
    }

    /// Set up the Workspace Bot with `permissions` (`null` keeps the defaults).
    pub(super) async fn setup(&self, permissions: Value) {
        let mut params = json!({"workspaceId":self.workspace,"execution":{"provider":"codex"}});
        if !permissions.is_null() {
            params["permissions"] = permissions;
        }
        self.rpc("bot.setup.request", params).await;
    }

    pub(super) async fn permissions(&self, permissions: Value) {
        self.rpc(
            "bot.permissions.update.request",
            json!({"workspaceId":self.workspace,"permissions":permissions}),
        )
        .await;
    }

    pub(super) async fn bot(&self, method: &str) {
        self.rpc(method, json!({"workspaceId":self.workspace}))
            .await;
    }

    /// Simulate opening the current Bot's hidden session of a conversation it does not have.
    pub(super) fn bot_credential(&self) -> String {
        self.bot_credential_in(ConversationId::generate())
    }

    /// Open the current Bot's `owner` conversation and return its id.
    pub(super) async fn conversation(&self) -> ConversationId {
        let opened = self
            .rpc(
                "bot.conversation.open.request",
                json!({"workspaceId":self.workspace}),
            )
            .await;
        serde_json::from_value(opened["conversation"]["id"].clone()).unwrap()
    }

    /// Simulate opening the current Bot's hidden session of `conversation`.
    pub(super) fn bot_credential_in(&self, conversation: ConversationId) -> String {
        let bot = self
            .store
            .catalog()
            .unwrap()
            .bots
            .into_iter()
            .find(|bot| {
                bot.workspace_id == self.workspace && !matches!(bot.state, BotState::Retired { .. })
            })
            .unwrap();
        let owner = serde_json::to_value(SessionOwner {
            bot_id: bot.id,
            conversation_id: conversation,
            workspace_id: self.workspace.clone(),
        })
        .unwrap();
        self.issue(BOT_SESSION, true, Some(&owner), &BTreeMap::new())
    }

    /// Simulate opening the native session of Bot-scoped Agent `agent_id`.
    pub(super) fn subtree_credential(&self, agent_id: &str) -> String {
        let labels = BTreeMap::from([(BOT_SCOPE_LABEL.to_owned(), self.workspace.clone())]);
        self.issue(agent_id, false, None, &labels)
    }

    fn issue(
        &self,
        agent_id: &str,
        internal: bool,
        owner: Option<&Value>,
        labels: &BTreeMap<String, String>,
    ) -> String {
        match self.scopes.access(&McpSubject {
            agent_id,
            internal,
            owner,
            labels,
        }) {
            McpAccess::Scoped { token, .. } => token.expose_secret().to_owned(),
            other => panic!("expected a scoped credential, got {other:?}"),
        }
    }

    /// Start an Agent in the root Workspace through the host RPC, outside MCP label rules.
    pub(super) async fn agent(&self, labels: Value) -> String {
        self.agent_in(&self.workspace, &self.cwd, labels, None)
            .await
    }

    /// Start an Agent in `workspace` at `cwd` through the host RPC, sending `prompt` if given.
    pub(super) async fn agent_in(
        &self,
        workspace: &str,
        cwd: &std::path::Path,
        labels: Value,
        prompt: Option<&str>,
    ) -> String {
        let mut params = json!({"workspaceId":workspace,"labels":labels,
            "config":{"provider":"codex","model":"offline-model","cwd":cwd,
                "title":"Scoped Agent"}});
        if let Some(prompt) = prompt {
            params["initialPrompt"] = json!(prompt);
        }
        let created = self.rpc("agent.create.request", params).await;
        created["agent"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{created}"))
            .to_owned()
    }

    /// Start a terminal in `workspace` at `cwd` through the host RPC.
    pub(super) async fn terminal(&self, workspace: &str, cwd: &std::path::Path) -> String {
        let created = self
            .rpc(
                "terminal.create.request",
                json!({"workspaceId":workspace,"cwd":cwd}),
            )
            .await;
        created["terminal"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{created}"))
            .to_owned()
    }

    pub(super) async fn subtree_agent(&self) -> String {
        self.agent(json!({BOT_SCOPE_LABEL:self.workspace})).await
    }

    pub(super) async fn post(
        &self,
        bearer: &str,
        caller: Option<&str>,
        method: &str,
        params: Value,
    ) -> reqwest::Response {
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(self.fixture.url(crate::mcp::PATH))
            .bearer_auth(bearer)
            .header("accept", "application/json, text/event-stream")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}));
        if let Some(caller) = caller {
            request = request.header("x-ait-agent-id", caller);
        }
        request.send().await.unwrap()
    }

    pub(super) async fn call(
        &self,
        bearer: &str,
        caller: Option<&str>,
        tool: &str,
        arguments: Value,
    ) -> Value {
        self.post(
            bearer,
            caller,
            "tools/call",
            json!({"name":tool,"arguments":arguments}),
        )
        .await
        .json()
        .await
        .unwrap()
    }

    pub(super) async fn tools(&self, bearer: &str) -> BTreeSet<String> {
        let listed: Value = self
            .post(bearer, None, "tools/list", json!({}))
            .await
            .json()
            .await
            .unwrap();
        listed["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("{listed}"))
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    }

    pub(super) async fn listing_status(&self, bearer: &str) -> StatusCode {
        self.post(bearer, None, "tools/list", json!({}))
            .await
            .status()
    }

    pub(super) async fn stop(self) {
        self.fixture.stop().await;
    }
}

pub(super) fn succeeded(response: &Value) -> bool {
    response["result"]["isError"] == false
}

pub(super) fn rejected(response: &Value, message: &str) {
    assert_eq!(response["error"]["code"], -32602, "{response}");
    assert_eq!(response["error"]["message"], message, "{response}");
}

pub(super) fn codex_modes(modes: &[&str]) -> Value {
    json!({"groups":["observe","agents","stop","schedules"],"modes":{"codex":modes}})
}

/// Assert that a tool call failed with the scope violation message.
pub(super) fn forbidden(response: &Value) {
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["content"][0]["text"],
        model::ErrorCode::Forbidden.message(),
        "{response}"
    );
}

/// The structured tool result of a successful call.
pub(super) fn structured(response: &Value) -> &Value {
    assert!(succeeded(response), "{response}");
    &response["result"]["structuredContent"]
}
