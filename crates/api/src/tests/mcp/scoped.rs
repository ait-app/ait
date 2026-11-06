//! Bot and subtree credentials on the Agent MCP route: recognition, caller context and groups.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use ::bot::mcp::Scopes;
use ::bot::ports::{Credentials, Executor};
use ::bot::service::{Bots, Dependencies};
use domain::ConversationId;
use domain::bot::permissions::{BotPermissions, ToolGroup};
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
use provider::Providers;
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
const BOT_SESSION: &str = "bot-hidden-session";

/// Bot conversations and schedules never run in these tests.
#[derive(Debug)]
struct Idle;

#[async_trait::async_trait]
impl Executor for Idle {
    async fn execute(&self, _: SessionOwner, _: Action) -> Result<Value, model::ErrorCode> {
        Ok(json!({"status":"idle","lastMessage":null}))
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

/// A served API with Bot, schedule and offline Codex provider services for one Workspace.
struct Scoped {
    fixture: Fixture,
    scopes: Arc<Scopes>,
    store: Arc<FileStore>,
    workspace: String,
    cwd: PathBuf,
    _root: tempfile::TempDir,
}

struct Registries {
    projects: FileBackedProjectRegistry,
    workspaces: FileBackedWorkspaceRegistry,
    agents: FileBackedAgentRuntimeRegistry,
}

fn provider(base: &std::path::Path, registries: &Registries) -> provider::Service {
    let data = base.join("data");
    let codex = base.join("codex");
    symlink(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../provider/tests/fixtures/codex_app_server.py"),
        &codex,
    )
    .unwrap();
    let mut manager = AgentManager::new(Box::new(registries.agents.clone())).with_timeline(
        provider::storage::timeline::Timeline::open(&data.join("timeline.sqlite3")).unwrap(),
    );
    Providers::configured(&data, |name| {
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
    async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        // Canonical paths keep macOS temporary directories under `/var` consistent.
        let base = root.path().canonicalize().unwrap();
        let data = base.join("data");
        let cwd = base.join("work");
        for directory in [&data, &cwd] {
            std::fs::create_dir_all(directory).unwrap();
        }
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
        let workspace = directory
            .open_workspace(cwd.to_str().unwrap(), "2026-11-06T00:00:00Z")
            .unwrap()
            .workspace_id;
        let scopes = Scopes::new();
        let store = Arc::new(FileStore::new(data.join("bots")));
        let bots = Bots::spawn(Dependencies {
            store: store.clone(),
            workspaces: Arc::new(registries.workspaces.clone()),
            projects: Arc::new(registries.projects.clone()),
            executor: Arc::new(Idle),
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
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut api = Api::new(
            listener.local_addr().unwrap(),
            "stable".to_owned(),
            "instance".to_owned(),
            TOKEN.into(),
            Services {
                bot: Some(bots),
                provider: Some(provider(&base, &registries)),
                schedule: Some(schedules),
                ..Services::default()
            },
        )
        .unwrap();
        let state = Arc::get_mut(&mut api.shared).unwrap();
        Arc::get_mut(&mut state.metadata).unwrap().directory = Some(shared_service(directory));
        let api = api.with_agent_mcp_token(AGENT_MCP_TOKEN.into()).unwrap();
        Self {
            fixture: Fixture::serve(listener, api),
            scopes,
            store,
            workspace,
            cwd,
            _root: root,
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        rpc::execute(&self.fixture.api.shared, method, params)
            .await
            .unwrap_or_else(|error| panic!("{method}: {error:?}"))
    }

    /// Set up the Workspace Bot with `permissions` (`null` keeps the defaults).
    async fn setup(&self, permissions: Value) {
        let mut params = json!({"workspaceId":self.workspace,"execution":{"provider":"codex"}});
        if !permissions.is_null() {
            params["permissions"] = permissions;
        }
        self.rpc("bot.setup.request", params).await;
    }

    async fn permissions(&self, permissions: Value) {
        self.rpc(
            "bot.permissions.update.request",
            json!({"workspaceId":self.workspace,"permissions":permissions}),
        )
        .await;
    }

    async fn bot(&self, method: &str) {
        self.rpc(method, json!({"workspaceId":self.workspace}))
            .await;
    }

    /// Simulate opening the current Bot's hidden conversation session.
    fn bot_credential(&self) -> String {
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
            conversation_id: ConversationId::generate(),
            workspace_id: self.workspace.clone(),
        })
        .unwrap();
        self.issue(BOT_SESSION, true, Some(&owner), &BTreeMap::new())
    }

    /// Simulate opening the native session of Bot-scoped Agent `agent_id`.
    fn subtree_credential(&self, agent_id: &str) -> String {
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

    /// Start an Agent in the Workspace through the host RPC, outside MCP label rules.
    async fn agent(&self, labels: Value) -> String {
        let created = self
            .rpc(
                "agent.create.request",
                json!({"workspaceId":self.workspace,"labels":labels,
                    "config":{"provider":"codex","model":"offline-model",
                        "cwd":self.cwd,"title":"Scoped Agent"}}),
            )
            .await;
        created["agent"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{created}"))
            .to_owned()
    }

    async fn subtree_agent(&self) -> String {
        self.agent(json!({BOT_SCOPE_LABEL:self.workspace})).await
    }

    async fn post(
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

    async fn call(
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

    async fn tools(&self, bearer: &str) -> BTreeSet<String> {
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

    async fn listing_status(&self, bearer: &str) -> StatusCode {
        self.post(bearer, None, "tools/list", json!({}))
            .await
            .status()
    }

    async fn stop(self) {
        self.fixture.stop().await;
    }
}

fn succeeded(response: &Value) -> bool {
    response["result"]["isError"] == false
}

fn rejected(response: &Value, message: &str) {
    assert_eq!(response["error"]["code"], -32602, "{response}");
    assert_eq!(response["error"]["message"], message, "{response}");
}

fn codex_modes(modes: &[&str]) -> Value {
    json!({"groups":["observe","agents","stop","schedules"],"modes":{"codex":modes}})
}

#[tokio::test]
async fn scoped_credential_lists_only_granted_tools() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let host = scoped.tools(TOKEN).await;
    let bot = scoped.tools(&scoped.bot_credential()).await;
    for granted in [
        "create_agent",
        "list_schedules",
        "cancel_agent",
        "list_agents",
    ] {
        assert!(bot.contains(granted), "{granted}: {bot:?}");
    }
    assert!(host.contains("respond_to_permission"));
    for withheld in [
        "respond_to_permission",
        "archive_workspace",
        "rename_workspace",
        "list_workspaces",
        "create_terminal",
    ] {
        assert!(!bot.contains(withheld), "{withheld}");
    }
    let defaults = BotPermissions::default();
    let expected: BTreeSet<_> = host
        .iter()
        .filter(|tool| defaults.allows(tool, true))
        .cloned()
        .collect();
    assert_eq!(bot, expected);
    scoped.stop().await;
}

#[tokio::test]
async fn bot_calls_with_session_header_resolve_root_context() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let credential = scoped.bot_credential();
    let header = Some(BOT_SESSION);
    let schedules = scoped
        .call(&credential, header, "list_schedules", json!({}))
        .await;
    assert!(succeeded(&schedules), "{schedules}");
    let provider = scoped
        .call(
            &credential,
            header,
            "inspect_provider",
            json!({"provider":"codex"}),
        )
        .await;
    assert!(succeeded(&provider), "{provider}");
    let other = scoped.agent(json!({})).await;
    rejected(
        &scoped
            .call(&credential, Some(&other), "list_schedules", json!({}))
            .await,
        "Caller does not match credential",
    );
    scoped.stop().await;
}

#[tokio::test]
async fn permission_change_applies_on_next_request() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto", "full-access"])).await;
    let child = scoped.subtree_agent().await;
    let credential = scoped.subtree_credential(&child);
    let create = |mode: Option<&str>| {
        let mut arguments = json!({"provider":"codex/offline-model","title":"Grandchild",
            "initialPrompt":"work"});
        if let Some(mode) = mode {
            arguments["settings"] = json!({"modeId":mode});
        }
        arguments
    };
    let full = scoped
        .call(
            &credential,
            None,
            "create_agent",
            create(Some("full-access")),
        )
        .await;
    assert!(succeeded(&full), "{full}");
    scoped
        .permissions(json!({"groups":["observe","stop"],"modes":{"codex":["auto","full-access"]}}))
        .await;
    rejected(
        &scoped
            .call(&credential, None, "create_agent", create(None))
            .await,
        "Unknown or unavailable tool",
    );
    scoped
        .permissions(codex_modes(&["auto", "full-access"]))
        .await;
    let restored = scoped
        .call(&credential, None, "create_agent", create(None))
        .await;
    assert!(succeeded(&restored), "{restored}");
    scoped.permissions(codex_modes(&["auto"])).await;
    let capped = scoped
        .call(
            &credential,
            None,
            "create_agent",
            create(Some("full-access")),
        )
        .await;
    assert_eq!(capped["result"]["isError"], true, "{capped}");
    assert_eq!(
        capped["result"]["content"][0]["text"],
        model::ErrorCode::Forbidden.message()
    );
    scoped.stop().await;
}

#[tokio::test]
async fn shared_token_rejected_for_subtree_caller() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let child = scoped.subtree_agent().await;
    let external = scoped.agent(json!({})).await;
    for token in [AGENT_MCP_TOKEN, TOKEN] {
        rejected(
            &scoped
                .call(token, Some(&child), "list_agents", json!({}))
                .await,
            "Bot-scoped agents must use their own credential",
        );
        let listed = scoped.call(token, None, "list_agents", json!({})).await;
        assert!(succeeded(&listed), "{listed}");
        let inherited = scoped
            .call(token, Some(&external), "list_agents", json!({}))
            .await;
        assert!(succeeded(&inherited), "{inherited}");
    }
    scoped.stop().await;
}

#[tokio::test]
async fn caller_header_must_match_scoped_credential() {
    let scoped = Scoped::start().await;
    scoped.setup(codex_modes(&["auto"])).await;
    let child = scoped.subtree_agent().await;
    let other = scoped.subtree_agent().await;
    let credential = scoped.subtree_credential(&child);
    rejected(
        &scoped
            .call(&credential, Some(&other), "list_agents", json!({}))
            .await,
        "Caller does not match credential",
    );
    let created = scoped
        .call(
            &credential,
            None,
            "create_agent",
            json!({"provider":"codex/offline-model","title":"Grandchild","initialPrompt":"work"}),
        )
        .await;
    assert!(succeeded(&created), "{created}");
    let grandchild = created["result"]["structuredContent"]["agent"]["id"].clone();
    let status = scoped
        .call(
            TOKEN,
            None,
            "get_agent_status",
            json!({"agentId":grandchild}),
        )
        .await;
    let snapshot = &status["result"]["structuredContent"]["snapshot"];
    assert_eq!(snapshot["labels"]["paseo.parent-agent-id"], child);
    assert_eq!(snapshot["workspaceId"], scoped.workspace);
    scoped.stop().await;
}

#[tokio::test]
async fn scoped_credential_is_limited_to_the_mcp_route() {
    let scoped = Scoped::start().await;
    scoped.setup(Value::Null).await;
    let credential = scoped.bot_credential();
    let subtree = scoped.subtree_credential(&scoped.subtree_agent().await);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for path in ["/v1/server/info", "/v1/ws"] {
        let status = client
            .get(scoped.fixture.url(path))
            .bearer_auth(&credential)
            .send()
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_eq!(scoped.listing_status(&credential).await, StatusCode::OK);
    scoped.bot("bot.pause.request").await;
    assert_eq!(
        scoped.listing_status(&credential).await,
        StatusCode::UNAUTHORIZED
    );
    scoped.bot("bot.resume.request").await;
    assert_eq!(
        scoped.listing_status(&credential).await,
        StatusCode::UNAUTHORIZED
    );
    let reissued = scoped.bot_credential();
    assert_eq!(scoped.listing_status(&reissued).await, StatusCode::OK);
    scoped.bot("bot.remove.request").await;
    assert_eq!(
        scoped.listing_status(&reissued).await,
        StatusCode::UNAUTHORIZED
    );
    let observe = BotPermissions::observe_only();
    let host = scoped.tools(TOKEN).await;
    let remaining = scoped.tools(&subtree).await;
    assert!(!remaining.is_empty());
    assert!(remaining.iter().all(|tool| observe.allows(tool, false)));
    assert_eq!(
        remaining,
        host.iter()
            .filter(|tool| domain::bot::permissions::tool_access(tool)
                == domain::bot::permissions::ToolAccess::Group(ToolGroup::Observe))
            .cloned()
            .collect()
    );
    scoped.setup(Value::Null).await;
    assert_eq!(
        scoped.listing_status(&reissued).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        scoped.listing_status(&scoped.bot_credential()).await,
        StatusCode::OK
    );
    scoped.stop().await;
}
