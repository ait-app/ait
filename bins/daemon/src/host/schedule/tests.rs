use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bot::mcp::Scopes;
use bot::ports::{Credentials, Executor};
use bot::service::{Bots, Dependencies};
use chrono::{DateTime, Utc};
use domain::ConversationId;
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_SCOPE_LABEL, SessionOwner};
use domain::schedule::Schedule;
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

use super::{AgentRpc, AgentRunner, REVOKED, available_agent, recover_interrupted, settle};
use crate::host::notifications::{Notifier, SCHEDULE_RUN_ENDED_LABEL};

const AGENT: &str = "11111111-1111-4111-8111-111111111111";

/// Agent RPCs answered as an idle Agent that finishes every prompt, recording each call.
#[derive(Debug, Default)]
struct Recorded {
    calls: Mutex<Vec<(String, Value)>>,
    /// Workspace of the existing heartbeat target Agent.
    workspace: String,
    /// Whether every prompt stops at a permission request instead of finishing.
    asks_permission: AtomicBool,
    /// Whether a turn was cancelled; its end is then reported after `turn_end_delay` waits.
    cancelled: AtomicBool,
    /// Completion waits after a cancellation that still report the cancelled turn running, as
    /// adapters whose turn end arrives asynchronously do.
    turn_end_delay: AtomicUsize,
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
            "agent.cancel.request" => {
                self.cancelled.store(true, Ordering::Relaxed);
                json!({})
            }
            "agent.finish.wait.request"
                if self.cancelled.load(Ordering::Relaxed)
                    && self
                        .turn_end_delay
                        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                            left.checked_sub(1)
                        })
                        .is_ok() =>
            {
                json!({"status":"running","final":{"status":"running"}})
            }
            "agent.finish.wait.request" if self.asks_permission.load(Ordering::Relaxed) => {
                json!({"status":"permission","final":{"pendingPermissions":[{"id":"p"}]}})
            }
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

/// A Workspace directory over registries stored in `data`, with those registries.
fn directory(
    data: &std::path::Path,
) -> (
    Directory,
    FileBackedProjectRegistry,
    FileBackedWorkspaceRegistry,
) {
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
    (directory, projects, workspaces)
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
        let (directory, projects, workspaces) = directory(&data);
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
            image_directory: None,
        })
        .unwrap();
        let agents = Arc::new(Recorded {
            workspace: workspace.clone(),
            ..Recorded::default()
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
            notifier: Notifier::current(bots.clone()),
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
        self.schedule_from(target, scoped, None).await
    }

    /// Create a schedule with `target`, in the Bot scope when `scoped`, started from
    /// `conversation`.
    async fn schedule_from(
        &self,
        target: Value,
        scoped: bool,
        conversation: Option<ConversationId>,
    ) -> String {
        let mut params = json!({"workspaceId":self.workspace,"prompt":"report",
            "cadence":{"type":"cron","expression":"0 9 * * *"},"target":target});
        if scoped {
            params["botScope"] = json!(self.workspace);
        }
        if let Some(conversation) = conversation {
            params["botConversation"] = json!(conversation);
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

#[tokio::test]
async fn each_bot_schedule_run_notifies_its_conversation_once() {
    let fixture = Fixture::start();
    fixture
        .bot("bot.setup.request", permissions(GROUPS, &["auto"]))
        .await;
    let conversation = ConversationId::generate();
    let task = fixture
        .schedule_from(codex(Some("auto")), true, Some(conversation))
        .await;
    let (succeeded, _) = fixture.run(&task).await;
    fixture
        .bot(
            "bot.permissions.update.request",
            permissions(&["observe"], &["auto"]),
        )
        .await;
    let (failed, _) = fixture.run(&task).await;
    assert_eq!(failed["error"], REVOKED, "{failed}");

    let labels = &fixture.agents.calls("agent.create.request")[0]["labels"];
    assert_eq!(
        labels[BOT_CONVERSATION_LABEL],
        json!(conversation),
        "{labels}"
    );
    let logged = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let listed = fixture
                .bots
                .execute(
                    "bot.notifications.list.request",
                    json!({"workspaceId":fixture.workspace}),
                )
                .await
                .unwrap();
            let entries = listed["notifications"].as_array().unwrap().clone();
            if entries.len() >= 2 {
                return entries;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let summary: Vec<_> = logged
        .iter()
        .map(|entry| {
            let notification = &entry["notification"];
            (
                notification["id"].as_str().unwrap().to_owned(),
                notification["kind"].clone(),
                notification["conversationId"].clone(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                format!("schedule:{task}:{}", failed["id"].as_str().unwrap()),
                json!("schedule_failed"),
                json!(conversation)
            ),
            (
                format!("schedule:{task}:{}", succeeded["id"].as_str().unwrap()),
                json!("schedule_succeeded"),
                json!(conversation)
            ),
        ]
    );
    assert_eq!(logged[1]["notification"]["agentId"], AGENT);
    fixture.stop().await;
}

/// Index of the last recorded call of `method`.
fn last_call(agents: &Recorded, method: &str) -> usize {
    agents
        .calls
        .lock()
        .unwrap()
        .iter()
        .rposition(|(called, _)| called == method)
        .unwrap()
}

#[tokio::test]
async fn a_kept_schedule_agent_is_marked_with_the_time_its_run_ended() {
    let fixture = Fixture::start();
    let kept = json!({"type":"new-agent","config":{"provider":"codex","archiveOnFinish":false}});
    let task = fixture.schedule(kept, false).await;
    let before = Utc::now();
    let (run, _) = fixture.run(&task).await;

    let updates = fixture.agents.calls("agent.update.request");
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert_eq!(updates[0]["agentId"], AGENT);
    let ended: DateTime<Utc> = updates[0]["labels"][SCHEDULE_RUN_ENDED_LABEL]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let settled: DateTime<Utc> = run["endedAt"].as_str().unwrap().parse().unwrap();
    assert!(before - chrono::TimeDelta::milliseconds(1) <= ended && ended <= settled);
    assert!(
        last_call(&fixture.agents, "agent.finish.wait.request")
            < last_call(&fixture.agents, "agent.update.request"),
        "marked only after the run's turn ended"
    );
    assert!(fixture.agents.calls("agent.archive.request").is_empty());

    // A permission request fails the run; the Agent is marked only after its turn is cancelled.
    fixture
        .agents
        .asks_permission
        .store(true, Ordering::Relaxed);
    let (failed, _) = fixture.run(&task).await;
    assert_eq!(failed["error"], "Scheduled agent is waiting for permission");
    assert_eq!(fixture.agents.calls("agent.update.request").len(), 2);
    assert!(
        last_call(&fixture.agents, "agent.cancel.request")
            < last_call(&fixture.agents, "agent.update.request")
    );
    fixture
        .agents
        .asks_permission
        .store(false, Ordering::Relaxed);

    // An archived run Agent needs no marker.
    let archived = fixture.schedule(codex(None), false).await;
    fixture.run(&archived).await;
    assert_eq!(fixture.agents.calls("agent.update.request").len(), 2);
    assert_eq!(fixture.agents.calls("agent.archive.request").len(), 1);
    fixture.stop().await;
}

#[test]
fn recovery_marks_the_kept_agent_of_an_interrupted_run_as_ended() {
    let root = tempfile::tempdir().unwrap();
    let (directory, _, _) = directory(root.path());
    let schedule = |id: &str, config: Value, agent: &str| -> Schedule {
        serde_json::from_value(json!({
            "id":id,"workspaceId":"wks_0123456789abcdef","prompt":"report",
            "cadence":{"type":"cron","expression":"0 9 * * *"},
            "target":{"type":"new-agent","config":config},"status":"active",
            "createdAt":"2026-11-12T00:00:00Z","updatedAt":"2026-11-12T00:00:00Z",
            "nextRunAt":null,"lastRunAt":null,"pausedAt":null,"expiresAt":null,"maxRuns":null,
            "runs":[{"id":"run","scheduledFor":"2026-11-12T08:00:00Z",
                "startedAt":"2026-11-12T08:00:00Z","endedAt":null,"status":"running",
                "agentId":agent,"workspaceId":null,"output":null,"error":null}]}))
        .unwrap()
    };
    let schedules = [
        schedule(
            "kept",
            json!({"provider":"codex","archiveOnFinish":false}),
            AGENT,
        ),
        schedule("archived", json!({"provider":"codex"}), "archived-agent"),
    ];
    let agents = Recorded::default();
    let before = Utc::now();

    recover_interrupted(&schedules, &agents, &directory).unwrap();

    let updates = agents.calls("agent.update.request");
    assert_eq!(updates.len(), 1, "{updates:?}");
    assert_eq!(updates[0]["agentId"], AGENT);
    let ended: DateTime<Utc> = updates[0]["labels"][SCHEDULE_RUN_ENDED_LABEL]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(before - chrono::TimeDelta::milliseconds(1) <= ended && ended <= Utc::now());
    let archived = agents.calls("agent.archive.request");
    assert_eq!(archived, [json!({"agentId":"archived-agent"})]);
}

#[tokio::test]
async fn a_kept_agent_is_marked_only_after_its_cancelled_turn_has_ended() {
    let fixture = Fixture::start();
    fixture
        .agents
        .asks_permission
        .store(true, Ordering::Relaxed);
    fixture.agents.turn_end_delay.store(1, Ordering::Relaxed);
    let kept = json!({"type":"new-agent","config":{"provider":"codex","archiveOnFinish":false}});
    let task = fixture.schedule(kept, false).await;

    let (failed, _) = fixture.run(&task).await;

    assert_eq!(failed["error"], "Scheduled agent is waiting for permission");
    let calls: Vec<String> = fixture
        .agents
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|(method, _)| method.clone())
        .collect();
    let cancel = calls
        .iter()
        .position(|method| method == "agent.cancel.request")
        .unwrap();
    let after: Vec<&str> = calls[cancel + 1..].iter().map(String::as_str).collect();
    // One wait still sees the cancelled turn running; the next sees it ended, then the marker.
    assert_eq!(
        after,
        [
            "agent.finish.wait.request",
            "agent.finish.wait.request",
            "agent.update.request"
        ]
    );
    assert_eq!(fixture.agents.turn_end_delay.load(Ordering::Relaxed), 0);
    fixture.stop().await;
}

#[tokio::test]
async fn waiting_for_a_cancelled_turn_gives_up_at_its_limit() {
    let agents = Recorded {
        cancelled: AtomicBool::new(true),
        turn_end_delay: AtomicUsize::new(usize::MAX),
        ..Recorded::default()
    };

    let settled = settle(&agents, AGENT, std::time::Duration::from_millis(100)).await;

    assert!(!settled);
    assert!(!agents.calls("agent.finish.wait.request").is_empty());
    agents.turn_end_delay.store(0, Ordering::Relaxed);
    assert!(settle(&agents, AGENT, std::time::Duration::from_millis(100)).await);
}
