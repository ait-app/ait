use std::sync::{Arc, Mutex};

use bot::mcp::Scopes;
use bot::ports::{Credentials, Executor};
use bot::service::{Bots, Dependencies};
use domain::bot::{BOT_SCOPE_LABEL, SessionOwner};
use filesystem::forge::local::forge::LocalForge;
use filesystem::git::local::provisioning::LocalDirectorySource;
use filesystem::git::local::worktrees::LocalManagedWorktrees;
use metadata::directory::service::directory::{Directory, DirectoryDependencies};
use metadata::worktrees::service::worktrees::Worktrees;
use persistence::storage::project_config::LocalProjectConfigStore;
use persistence::storage::project_icon::LocalProjectIconStore;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::bot_execution::Action;
use schedule::service::Schedules;
use secrecy::SecretString;
use serde_json::{Value, json};

use super::{AgentRpc, AgentRunner, REVOKED, available_agent};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";

/// Agent RPCs answered as an idle Agent that finishes every prompt, recording each call.
#[derive(Debug)]
struct Recorded {
    calls: Mutex<Vec<(String, Value)>>,
    /// Workspace of the existing heartbeat target Agent.
    workspace: String,
}

#[async_trait::async_trait]
impl AgentRpc for Recorded {
    async fn execute(
        &self,
        method: &str,
        params: Value,
    ) -> Result<Value, provider::rpc::ErrorCode> {
        self.calls.lock().unwrap().push((method.to_owned(), params));
        Ok(match method {
            "agent.create.request" => json!({"agentId":AGENT}),
            "agent.get.request" => json!({"agent":{"id":AGENT,"archivedAt":null,
                "workspaceId":self.workspace}}),
            "agent.message.send.request" => json!({"accepted":true}),
            "agent.finish.wait.request" => json!({"status":"idle","lastMessage":"done"}),
            _ => json!({}),
        })
    }
}

impl Recorded {
    /// Parameters of every recorded `method` call.
    fn calls(&self, method: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(called, _)| called == method)
            .map(|(_, params)| params.clone())
            .collect()
    }
}

/// Bot conversations never run in these tests.
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

/// A schedule worker running through `AgentRunner` in one Workspace with a Workspace Bot.
struct Fixture {
    agents: Arc<Recorded>,
    bots: Bots,
    schedules: Schedules,
    workspace: String,
    _root: tempfile::TempDir,
}

impl Fixture {
    fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let (data, cwd) = (base.join("data"), base.join("work"));
        for directory in [&data, &cwd] {
            std::fs::create_dir_all(directory).unwrap();
        }
        let projects = FileBackedProjectRegistry::new(data.join("projects.json"));
        let workspaces = FileBackedWorkspaceRegistry::new(data.join("workspaces.json"));
        let directory = Directory::new(DirectoryDependencies {
            projects: Box::new(projects.clone()),
            workspaces: Box::new(workspaces.clone()),
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
        let bots = Bots::spawn(Dependencies {
            store: Arc::new(persistence::storage::bot::FileStore::new(data.join("bots"))),
            workspaces: Arc::new(workspaces.clone()),
            projects: Arc::new(projects.clone()),
            executor: Arc::new(Idle),
            credentials: Arc::new(Idle),
            scopes: scopes.clone(),
        })
        .unwrap();
        let agents = Arc::new(Recorded {
            calls: Mutex::default(),
            workspace: workspace.clone(),
        });
        let worktrees = Worktrees::new(
            Box::new(projects),
            Box::new(workspaces),
            Box::new(LocalManagedWorktrees::new(
                data.join("worktrees"),
                Arc::new(LocalForge::new()),
            )),
            "test-server".to_owned(),
        );
        let runner = AgentRunner {
            execution: agents.clone(),
            directory,
            worktrees: Arc::new(Mutex::new(worktrees)),
            scopes,
        };
        let schedules = Schedules::spawn(
            Box::new(persistence::storage::schedule::FileStore::new(
                data.join("schedules.json"),
            )),
            Arc::new(runner),
        )
        .unwrap();
        Self {
            agents,
            bots,
            schedules,
            workspace,
            _root: root,
        }
    }

    async fn bot(&self, method: &str, permissions: Value) {
        let mut params = json!({"workspaceId":self.workspace,"permissions":permissions});
        if method == "bot.setup.request" {
            params["execution"] = json!({"provider":"codex"});
        }
        self.bots.execute(method, params).await.unwrap();
    }

    /// Create a schedule with `target`, in the Bot scope when `scoped`.
    async fn schedule(&self, target: Value, scoped: bool) -> String {
        let mut params = json!({"workspaceId":self.workspace,"prompt":"report",
            "cadence":{"type":"cron","expression":"0 9 * * *"},"target":target});
        if scoped {
            params["botScope"] = json!(self.workspace);
        }
        let created = self
            .schedules
            .execute("schedule.create.request", params)
            .await
            .unwrap();
        created["schedule"]["id"].as_str().unwrap().to_owned()
    }

    /// Run schedule `id` once and return the settled occurrence and the schedule status.
    async fn run(&self, id: &str) -> (Value, Value) {
        let settled = self
            .schedules
            .execute("schedule.run_once.request", json!({"scheduleId":id}))
            .await
            .unwrap();
        let schedule = &settled["schedule"];
        let run = schedule["runs"].as_array().unwrap().last().unwrap().clone();
        (run, schedule["status"].clone())
    }

    async fn stop(self) {
        self.schedules.shutdown().await.unwrap();
        self.bots.shutdown().await.unwrap();
    }
}

fn permissions(groups: &[&str], modes: &[&str]) -> Value {
    json!({"groups":groups,"modes":{"codex":modes}})
}

const GROUPS: &[&str] = &["observe", "agents", "stop", "schedules"];

fn codex(mode: Option<&str>) -> Value {
    let mut config = json!({"provider":"codex"});
    if let Some(mode) = mode {
        config["modeId"] = json!(mode);
    }
    json!({"type":"new-agent","config":config})
}

#[test]
fn observed_agents_cannot_be_scheduled_or_reused_as_existing_targets() {
    assert!(available_agent(
        &json!({"agent":{"management":"managed","archivedAt":null}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"management":"observed","archivedAt":null}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"archivedAt":null,"capabilities":{"supportsMessageSend":false}}})
    ));
    assert!(!available_agent(
        &json!({"agent":{"archivedAt":"2026-10-31T00:00:00Z"}})
    ));
    assert!(!available_agent(&json!({"agent":null})));
}

#[tokio::test]
async fn bot_schedule_agents_receive_derived_scope_and_mode_cap() {
    let fixture = Fixture::start();
    fixture
        .bot("bot.setup.request", permissions(GROUPS, &["read-only"]))
        .await;

    let scoped = fixture.schedule(codex(Some("read-only")), true).await;
    let (run, _) = fixture.run(&scoped).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let created = fixture.agents.calls("agent.create.request");
    let labels = &created[0]["labels"];
    assert_eq!(
        labels[BOT_SCOPE_LABEL],
        fixture.workspace.as_str(),
        "{labels}"
    );
    assert_eq!(labels["paseo.schedule-id"], scoped.as_str());
    assert_eq!(created[0]["config"]["modeId"], "read-only");

    // A Bot task whose stored mode was cleared runs in the configured mode.
    let cleared = fixture.schedule(codex(None), true).await;
    let (run, _) = fixture.run(&cleared).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    assert_eq!(
        fixture.agents.calls("agent.create.request")[1]["config"]["modeId"],
        "read-only"
    );

    let user = fixture.schedule(codex(None), false).await;
    let (run, _) = fixture.run(&user).await;
    assert_eq!(run["status"], "succeeded", "{run}");
    let created = fixture.agents.calls("agent.create.request");
    let labels = &created[2]["labels"];
    assert!(labels.get(BOT_SCOPE_LABEL).is_none(), "{labels}");
    assert_eq!(labels["paseo.schedule-id"], user.as_str());
    assert!(created[2]["config"].get("modeId").is_none());
    fixture.stop().await;
}

#[tokio::test]
async fn bot_schedule_run_rechecks_current_grant() {
    let fixture = Fixture::start();
    fixture
        .bot("bot.setup.request", permissions(GROUPS, &["auto"]))
        .await;
    let task = fixture.schedule(codex(Some("auto")), true).await;
    let heartbeat = fixture
        .schedule(json!({"type":"agent","agentId":AGENT}), true)
        .await;

    let revoked = [
        permissions(GROUPS, &["read-only"]),
        permissions(&["observe", "agents", "stop"], &["auto"]),
    ];
    for permissions in revoked {
        fixture
            .bot("bot.permissions.update.request", permissions)
            .await;
        let (run, status) = fixture.run(&task).await;
        assert_eq!(run["status"], "failed", "{run}");
        assert_eq!(run["error"], REVOKED);
        assert_eq!(status, "active");
        assert!(fixture.agents.calls("agent.create.request").is_empty());
    }
    let (run, status) = fixture.run(&heartbeat).await;
    assert_eq!(run["error"], REVOKED, "{run}");
    assert_eq!(status, "active");
    assert!(
        fixture
            .agents
            .calls("agent.message.send.request")
            .is_empty()
    );

    fixture
        .bot(
            "bot.permissions.update.request",
            permissions(GROUPS, &["auto"]),
        )
        .await;
    for id in [&task, &heartbeat] {
        let (run, _) = fixture.run(id).await;
        assert_eq!(run["status"], "succeeded", "{run}");
    }
    assert_eq!(fixture.agents.calls("agent.create.request").len(), 1);
    assert_eq!(fixture.agents.calls("agent.message.send.request").len(), 2);
    fixture.stop().await;
}
