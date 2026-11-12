use std::path::Path;
use std::time::Duration;

use bot::ports::{Credentials, Executor};
use bot::service::Dependencies;
use domain::ConversationId;
use domain::bot::SessionOwner;
use domain::workspace::registry::WorkspaceMutationContext;
use model::workspace::registry::{ProjectRegistry, WorkspaceRegistry};
use persistence::storage::agent_runtime::FileBackedAgentRuntimeRegistry;
use persistence::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use provider::service::bot_execution::Action;
use secrecy::SecretString;
use serde_json::json;

use super::*;

const WORKSPACE: &str = "wks_0123456789abcdef";
const AGENT: &str = "11111111-1111-4111-8111-111111111111";
const CONVERSATION: &str = "33333333-3333-4333-8333-333333333333";

fn record(fields: &Value) -> PersistedAgentRuntimeRecord {
    let mut record = json!({"id":AGENT,"provider":"codex","cwd":"/tmp","workspaceId":WORKSPACE,
        "createdAt":"2026-11-12T00:00:00Z","updatedAt":"2026-11-12T00:00:00Z","title":"Fix",
        "labels":{BOT_SCOPE_LABEL:WORKSPACE,BOT_CONVERSATION_LABEL:CONVERSATION}});
    for (key, value) in fields.as_object().unwrap() {
        record[key] = value.clone();
    }
    serde_json::from_value(record).unwrap()
}

const AT: &str = "2026-11-12T08:00:00Z";

#[test]
fn agent_attention_maps_to_a_deterministic_notification_with_its_origin() {
    let failed = record(&json!({"lastError":"boom","attentionTimestamp":AT}));
    let notification = agent_notification(&failed, AgentAttentionReason::Error, AT).unwrap();
    assert_eq!(notification.id, format!("agent:{AGENT}:{AT}"));
    assert_eq!(notification.workspace_id, WORKSPACE);
    assert_eq!(notification.scope.as_deref(), Some(WORKSPACE));
    assert_eq!(notification.kind, NotificationKind::AgentFailed);
    assert_eq!(notification.agent_id.as_deref(), Some(AGENT));
    assert_eq!(notification.title, "Agent \"Fix\" failed");
    assert_eq!(notification.summary.as_deref(), Some("boom"));
    assert_eq!(notification.at.to_rfc3339(), "2026-11-12T08:00:00+00:00");
    assert_eq!(
        notification.conversation_id.map(|id| id.to_string()),
        Some(CONVERSATION.to_owned())
    );

    let untitled = record(&json!({"title":null,"labels":{}}));
    let finished = agent_notification(&untitled, AgentAttentionReason::Finished, AT).unwrap();
    assert_eq!(finished.kind, NotificationKind::AgentFinished);
    assert_eq!(finished.title, "Agent finished");
    assert_eq!((finished.scope, finished.conversation_id), (None, None));
    assert!(finished.summary.is_none());
    let permission = agent_notification(&untitled, AgentAttentionReason::Permission, AT).unwrap();
    assert_eq!(permission.kind, NotificationKind::AgentPermission);
}

#[test]
fn internal_archived_stale_and_scheduled_attention_is_skipped() {
    let finished = AgentAttentionReason::Finished;
    for (fields, reason) in [
        (json!({"internal":true}), finished),
        (json!({"archivedAt":AT}), finished),
        (
            json!({"attentionTimestamp":"2026-11-12T09:00:00Z"}),
            finished,
        ),
        (json!({"workspaceId":null}), finished),
        (
            json!({"labels":{"paseo.schedule-run":"run"}}),
            AgentAttentionReason::Error,
        ),
        (json!({"labels":{"paseo.schedule-run":"run"}}), finished),
        // The runner fails the run and archives the Agent; its run result says why.
        (
            json!({"labels":{"paseo.schedule-run":"run"}}),
            AgentAttentionReason::Permission,
        ),
    ] {
        assert!(
            agent_notification(&record(&fields), reason, AT).is_none(),
            "{fields}"
        );
    }
    assert!(agent_notification(&record(&json!({})), finished, "yesterday").is_none());
    // Attention a client already cleared still notifies.
    assert!(agent_notification(&record(&json!({})), finished, AT).is_some());
}

#[test]
fn the_rescan_only_replays_recent_current_attention() {
    let now: DateTime<Utc> = "2026-11-12T08:00:00Z".parse().unwrap();
    let at = |minutes: i64| (now - TimeDelta::minutes(minutes)).to_rfc3339();
    let records: Vec<_> = [
        json!({"id":"a","requiresAttention":true,"attentionReason":"finished",
            "attentionTimestamp":at(1)}),
        json!({"id":"b","requiresAttention":true,"attentionReason":"permission",
            "attentionTimestamp":at(60 * 24)}),
        json!({"id":"c","requiresAttention":false,"attentionTimestamp":at(1)}),
        json!({"id":"d","requiresAttention":true,"attentionReason":"error",
            "attentionTimestamp":at(14),"internal":true}),
    ]
    .iter()
    .map(|fields| Arc::new(record(fields)))
    .collect();
    let replayed = recent_attention(&records, now);
    let ids: Vec<_> = replayed
        .iter()
        .map(|notification| notification.agent_id.as_deref().unwrap())
        .collect();
    assert_eq!(ids, ["a"]);
}

#[test]
fn terminal_attention_has_no_origin() {
    let now: DateTime<Utc> = AT.parse().unwrap();
    let payload = json!({"terminalId":"term","workspaceId":WORKSPACE,"title":"Terminal finished",
        "body":"build"});
    let notification = terminal_notification(&payload, now).unwrap();
    assert_eq!(
        notification.id,
        format!("terminal:term:{}", now.timestamp_millis())
    );
    assert_eq!(notification.kind, NotificationKind::TerminalAttention);
    assert_eq!(notification.summary.as_deref(), Some("build"));
    assert_eq!(
        (notification.scope, notification.conversation_id),
        (None, None)
    );
    assert!(terminal_notification(&json!({"terminalId":"term"}), now).is_none());
}

#[test]
fn each_settled_schedule_run_maps_to_one_notification() {
    let conversation: ConversationId = CONVERSATION.parse().unwrap();
    let mut schedule: Schedule = serde_json::from_value(json!({
        "id":"sched","workspaceId":WORKSPACE,"botScope":WORKSPACE,
        "botConversation":conversation,"name":"Nightly","prompt":"report",
        "cadence":{"type":"cron","expression":"0 9 * * *"},
        "target":{"type":"new-agent","config":{"provider":"codex"}},"status":"active",
        "createdAt":AT,"updatedAt":AT,"nextRunAt":null,"lastRunAt":null,"pausedAt":null,
        "expiresAt":null,"maxRuns":null,"runs":[]}))
    .unwrap();
    let run: Run = serde_json::from_value(json!({"id":"run","scheduledFor":AT,"startedAt":AT,
        "endedAt":AT,"status":"failed","agentId":null,"workspaceId":null,"output":null,
        "error":"Workspace Bot permissions no longer allow this schedule"}))
    .unwrap();
    let failed = schedule_notification(&schedule, &run).unwrap();
    assert_eq!(failed.id, "schedule:sched:run");
    assert_eq!(failed.kind, NotificationKind::ScheduleFailed);
    assert_eq!(failed.workspace_id, WORKSPACE);
    assert_eq!(failed.scope.as_deref(), Some(WORKSPACE));
    assert_eq!(failed.conversation_id, Some(conversation));
    assert_eq!(failed.title, "Schedule \"Nightly\" run failed");
    assert!(failed.summary.unwrap().contains("no longer allow"));

    let mut succeeded = run.clone();
    succeeded.status = RunStatus::Succeeded;
    succeeded.output = Some("é".repeat(SUMMARY_BYTES));
    succeeded.agent_id = Some(AGENT.to_owned());
    let notification = schedule_notification(&schedule, &succeeded).unwrap();
    assert_eq!(notification.kind, NotificationKind::ScheduleSucceeded);
    assert_eq!(notification.agent_id.as_deref(), Some(AGENT));
    assert_eq!(notification.summary.unwrap().len(), SUMMARY_BYTES);

    let mut running = run.clone();
    running.status = RunStatus::Running;
    assert!(schedule_notification(&schedule, &running).is_none());

    // A heartbeat's Agent reports its own turn; only a failed heartbeat run notifies.
    let mut heartbeat = schedule.clone();
    heartbeat.target = serde_json::from_value(json!({"type":"agent","agentId":AGENT})).unwrap();
    assert!(schedule_notification(&heartbeat, &succeeded).is_none());
    assert_eq!(
        schedule_notification(&heartbeat, &run).unwrap().kind,
        NotificationKind::ScheduleFailed
    );
    schedule.workspace_id = None;
    assert!(schedule_notification(&schedule, &run).is_none());
}

/// Bot sessions that end every wake turn quietly.
#[derive(Debug)]
struct Quiet;

#[async_trait::async_trait]
impl Executor for Quiet {
    async fn execute(&self, _: SessionOwner, action: Action) -> Result<Value, ErrorCode> {
        Ok(match action {
            Action::Create { .. } => json!({"created":true,"instructionDelivery":"system_prompt"}),
            Action::Send { .. } => json!({"accepted":true}),
            Action::Poll { .. } | Action::Status { .. } => {
                json!({"status":"idle","lastMessage":"NO_REPLY"})
            }
            Action::Cancel { .. } | Action::Close { .. } => json!({"ok":true}),
            Action::Release { .. } => json!({"released":true}),
            Action::Permission { .. } => json!({"resolved":true}),
        })
    }
}

impl Credentials for Quiet {
    fn resolve(&self, _: &str) -> Result<SecretString, ErrorCode> {
        Err(ErrorCode::BotIo)
    }
}

/// A Bot service for [`WORKSPACE`] plus an Agent registry, both under `root`.
async fn bots_at(root: &Path) -> (Bots, Arc<dyn AgentRuntimeRegistry>, ConversationId) {
    let cwd = root.to_str().unwrap();
    let projects = FileBackedProjectRegistry::new(root.join("projects.json"));
    projects
        .upsert(
            &serde_json::from_value(json!({"projectId":"project-1","rootPath":cwd,
            "kind":"non_git","displayName":"project","createdAt":AT,"updatedAt":AT,
            "archivedAt":null}))
            .unwrap(),
        )
        .unwrap();
    let workspaces = FileBackedWorkspaceRegistry::new(root.join("workspaces.json"));
    workspaces
        .upsert(
            &serde_json::from_value(json!({"workspaceId":WORKSPACE,"projectId":"project-1",
                "cwd":cwd,"kind":"local_checkout","displayName":"workspace","createdAt":AT,
                "updatedAt":AT,"archivedAt":null}))
            .unwrap(),
            WorkspaceMutationContext::default(),
        )
        .unwrap();
    let bots = Bots::spawn(Dependencies {
        store: Arc::new(persistence::storage::bot::FileStore::new(root.join("bots"))),
        workspaces: Arc::new(workspaces),
        projects: Arc::new(projects),
        executor: Arc::new(Quiet),
        credentials: Arc::new(Quiet),
        scopes: bot::mcp::Scopes::new(),
    })
    .unwrap();
    bots.execute(
        "bot.setup.request",
        json!({"workspaceId":WORKSPACE,"execution":{"provider":"test"}}),
    )
    .await
    .unwrap();
    let opened = bots
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .await
        .unwrap();
    let conversation = serde_json::from_value(opened["conversation"]["id"].clone()).unwrap();
    let agents: Arc<dyn AgentRuntimeRegistry> = Arc::new(FileBackedAgentRuntimeRegistry::new(
        root.join("agents.json"),
    ));
    (bots, agents, conversation)
}

async fn logged(bots: &Bots) -> Vec<Value> {
    bots.execute(
        "bot.notifications.list.request",
        json!({"workspaceId":WORKSPACE}),
    )
    .await
    .unwrap()["notifications"]
        .as_array()
        .unwrap()
        .clone()
}

/// Poll `ready` over the Bot's log until it holds, failing after ten seconds.
async fn until(bots: &Bots, ready: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let entries = logged(bots).await;
            if ready(&entries) {
                return entries;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap()
}

/// Every wake turn of `conversation` carrying notification `id`.
async fn wakes_of(bots: &Bots, conversation: ConversationId, id: &str) -> Vec<Value> {
    let history = bots
        .execute(
            "bot.history.get.request",
            json!({"workspaceId":WORKSPACE,"conversationId":conversation}),
        )
        .await
        .unwrap();
    history["turns"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|turn| turn["wake"]["notifications"][0]["id"] == id)
        .cloned()
        .collect()
}

#[tokio::test]
async fn focused_attention_wakes_the_origin_conversation_and_restarts_do_not_repeat_it() {
    let root = tempfile::tempdir().unwrap();
    let (bots, agents, conversation) = bots_at(root.path()).await;
    let now = Utc::now().to_rfc3339();
    // The Agent still requires attention, so every rescan offers it again.
    agents
        .upsert(&record(&json!({"labels":{BOT_SCOPE_LABEL:WORKSPACE,
            BOT_CONVERSATION_LABEL:conversation},"requiresAttention":true,
            "attentionReason":"finished","attentionTimestamp":now})))
        .unwrap();
    let events = SessionEvents::default();
    let viewer = events.connect();
    viewer
        .heartbeat(
            serde_json::from_value(json!({"deviceType":"web","focusedAgentId":AGENT,
                "appVisible":true,"lastActivityAt":now}))
            .unwrap(),
        )
        .unwrap();
    let notifier = Notifier::current(bots.clone()).unwrap();
    notifier.forward_attention(&events, agents.clone());

    events.publish(
        SessionEventKind::AgentAttention,
        &json!({"agentId":AGENT,"reason":"finished","timestamp":now}),
    );

    let id = format!("agent:{AGENT}:{now}");
    let entries = until(&bots, |entries| {
        entries.iter().any(|entry| entry["wake"]["state"] == "woke")
    })
    .await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["notification"]["id"], id.as_str());
    assert_eq!(entries[0]["wake"]["conversationId"], json!(conversation));
    let woke = wakes_of(&bots, conversation, &id).await;
    assert_eq!(woke.len(), 1);
    assert_eq!(woke[0]["id"], entries[0]["wake"]["turnId"]);
    bots.shutdown().await.unwrap();

    // A real restart: the start-up rescan replays the woken notification, which stays one entry.
    let (restarted, agents, _) = bots_at(root.path()).await;
    let recent = (Utc::now() - TimeDelta::minutes(1)).to_rfc3339();
    let old = (Utc::now() - TimeDelta::hours(2)).to_rfc3339();
    for (agent, timestamp) in [
        ("22222222-2222-4222-8222-222222222222", &recent),
        ("44444444-4444-4444-8444-444444444444", &old),
    ] {
        agents
            .upsert(&record(&json!({"id":agent,"requiresAttention":true,
                "attentionReason":"permission","attentionTimestamp":timestamp})))
            .unwrap();
    }
    rescan(&restarted, &agents).await;
    rescan(&restarted, &agents).await;
    let ids: Vec<String> = logged(&restarted)
        .await
        .iter()
        .map(|entry| entry["notification"]["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        ids,
        [
            format!("agent:22222222-2222-4222-8222-222222222222:{recent}"),
            id.clone()
        ]
    );
    // Past the coalescing window, the replay has still not woken the conversation again.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(wakes_of(&restarted, conversation, &id).await.len(), 1);
    assert_eq!(logged(&restarted).await[1]["wake"]["state"], "woke");
    restarted.shutdown().await.unwrap();
}
