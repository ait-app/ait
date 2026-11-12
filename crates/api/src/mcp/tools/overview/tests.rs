use std::collections::BTreeSet;

use std::sync::LazyLock;

use super::*;

static CHECKOUT: LazyLock<Value> = LazyLock::new(|| json!({"currentBranch":"main","isDirty":true}));
static MEMORY: LazyLock<Value> = LazyLock::new(|| json!({"text":"hello","revision":"r0"}));
static NONE: Value = Value::Null;

fn agent(index: usize, workspace: &str, scoped: bool) -> Value {
    let labels = if scoped {
        json!({BOT_SCOPE_LABEL:"wks_root"})
    } else {
        json!({})
    };
    json!({"id":format!("agent-{index}"),"title":"T".repeat(60),"provider":"codex","status":"idle",
        "workspaceId":workspace,"labels":labels,"requiresAttention":false,
        "updatedAt":"2026-11-06T00:00:00Z","lastMessage":"m".repeat(4096)})
}

fn sources<'a>(
    own: &'a [Value],
    all: &'a [Value],
    schedules: &'a [Value],
    workspaces: &'a BTreeSet<String>,
    permissions: &'a BotPermissions,
) -> Sources<'a> {
    Sources {
        root: "wks_root",
        checkout: &CHECKOUT,
        own,
        all,
        all_complete: true,
        schedules,
        workspaces,
        memory: &MEMORY,
        notifications: &NONE,
        tickets: &NONE,
        conversation: None,
        permissions,
    }
}

/// A `bot.notifications.list.request` result with `count` entries, newest first.
fn listed(count: u64) -> Value {
    let entries: Vec<Value> = (1..=count)
        .rev()
        .map(|sequence| {
            json!({"sequence":sequence,"admittedAt":"2026-11-12T00:00:00Z",
                "wake":{"state":"woke","conversationId":"c","turnId":"t"},
                "notification":{"id":format!("agent:a{sequence}:t"),"workspaceId":"wks_root",
                    "scope":"wks_root","kind":"agent_failed","agentId":format!("a{sequence}"),
                    "scheduleId":null,"runId":null,"terminalId":null,"title":"Agent failed",
                    "summary":"s".repeat(2048),"at":"2026-11-12T00:00:00Z",
                    "conversationId":"c"}})
        })
        .collect();
    json!({"headSequence":count,"seenSequence":null,"wakeScope":"own","notifications":entries})
}

#[test]
fn workspace_overview_is_bounded_and_scoped() {
    let own: Vec<_> = (0..50)
        .map(|index| agent(index, "wks_root", true))
        .collect();
    let mut all = own.clone();
    all.push(agent(90, "wks_root", false));
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let schedules = [
        json!({"id":"s1","workspaceId":"wks_root","botScope":"wks_root","name":"n","status":"active"}),
        json!({"id":"s2","workspaceId":"wks_root","name":"user","status":"active"}),
        json!({"id":"foreign","workspaceId":"wks_other","botScope":"wks_root","status":"active"}),
    ];
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &all, &schedules, &workspaces, &permissions));
    let text = overview.to_string();
    assert!(text.len() <= MAX_BYTES, "{}", text.len());
    assert_eq!(overview["truncated"], true);
    assert_eq!(overview["botAgents"].as_array().unwrap().len(), 20);
    assert!(!text.contains("foreign"));
    assert!(overview["notifications"].is_null());
    assert_eq!(overview["otherAgents"]["count"], 1);
    assert_eq!(overview["schedules"]["otherCount"], 1);
    assert_eq!(overview["schedules"]["bot"].as_array().unwrap().len(), 1);
    assert_eq!(overview["memory"], json!({"revision":"r0","bytes":5}));
    assert_eq!(overview["workspace"]["branch"], "main");
    assert_eq!(overview["permissions"]["groups"][0], "observe");
}

#[test]
fn small_overview_keeps_last_messages_and_is_not_truncated() {
    let own = [agent(0, "wks_root", true), agent(1, "wks_root", true)];
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &own, &[], &workspaces, &permissions));
    assert_eq!(overview["truncated"], false);
    assert_eq!(
        overview["botAgents"][0]["lastMessage"]
            .as_str()
            .unwrap()
            .len(),
        MAX_MESSAGE
    );
}

#[test]
fn oversized_documents_lose_last_messages_before_entries() {
    let own: Vec<_> = (0..20)
        .map(|index| agent(index, "wks_root", true))
        .collect();
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &own, &[], &workspaces, &permissions));
    assert!(overview.to_string().len() <= MAX_BYTES);
    assert_eq!(overview["botAgents"].as_array().unwrap().len(), 20);
    assert!(overview["botAgents"][0].get("lastMessage").is_none());
    assert_eq!(overview["truncated"], false);
}

#[test]
fn unseen_notifications_are_projected_newest_first_within_the_bound() {
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let few = listed(2);
    let mut small = sources(&[], &[], &[], &workspaces, &permissions);
    small.notifications = &few;
    let overview = project(&small);
    assert_eq!(overview["truncated"], false);
    assert_eq!(
        overview["notifications"][0],
        json!({"sequence":2,"wake":"woke","kind":"agent_failed","title":"Agent failed",
            "workspaceId":"wks_root","agentId":"a2","at":"2026-11-12T00:00:00Z",
            "summary":"s".repeat(MAX_SUMMARY)})
    );
    assert_eq!(overview["notifications"][1]["sequence"], 1);

    let many = listed(MAX_NOTIFICATIONS as u64 + 1);
    let own: Vec<_> = (0..20)
        .map(|index| agent(index, "wks_root", true))
        .collect();
    let mut large = sources(&own, &own, &[], &workspaces, &permissions);
    large.notifications = &many;
    let overview = project(&large);
    assert!(overview.to_string().len() <= MAX_BYTES);
    assert_eq!(overview["truncated"], true, "the extra entry was dropped");
    let notifications = overview["notifications"].as_array().unwrap();
    assert_eq!(notifications.len(), MAX_NOTIFICATIONS);
    assert_eq!(notifications[0]["sequence"], MAX_NOTIFICATIONS + 1);
    assert!(
        notifications
            .iter()
            .all(|entry| entry.get("summary").is_none())
    );
    assert!(overview["botAgents"][0].get("lastMessage").is_none());

    let empty = json!({"headSequence":0,"seenSequence":null,"notifications":[]});
    small.notifications = &empty;
    assert_eq!(project(&small)["notifications"], json!([]));
}

/// A `bot.ticket.list.request` result with `count` open tickets of 16 Agents each, the first
/// asked in `CONVERSATION`, assigned `agent-0` to `agent-15`.
fn tickets(count: usize) -> Value {
    let tickets: Vec<Value> = (0..count)
        .map(|index| {
            let assignments: Vec<Value> = (0..16)
                .map(|agent| {
                    json!({"agentId":format!("agent-{agent}"),"workspaceId":"wks_root",
                    "assignedAt":"2026-11-13T00:00:00Z"})
                })
                .collect();
            json!({"id":format!("ticket-{index}"),"title":"T".repeat(256),"status":"waiting",
                "waiting":{"reason":"blocked","agentId":"agent-0","since":"2026-11-13T00:00:00Z"},
                "conversationId":if index == 0 { CONVERSATION } else { "other" },
                "createdAt":"2026-11-13T00:00:00Z","updatedAt":"2026-11-13T00:00:00Z",
                "closedAt":null,"outcome":null,"assignments":assignments,
                "latestReport":{"sequence":3,"at":"2026-11-13T00:00:00Z","source":"agent",
                    "kind":"blocked","agentId":"agent-0","text":"r".repeat(2048),
                    "status":"waiting"}})
        })
        .collect();
    json!({"tickets":tickets})
}

const CONVERSATION: &str = "33333333-3333-4333-8333-333333333333";

#[test]
fn open_tickets_join_live_agent_status_and_mark_the_calling_conversation() {
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let mut running = agent(0, "wks_root", true);
    running["status"] = json!("running");
    running["requiresAttention"] = json!(true);
    let own = [running];
    let listed = tickets(2);
    let mut small = sources(&own, &own, &[], &workspaces, &permissions);
    small.tickets = &listed;
    small.conversation = Some(CONVERSATION.parse().unwrap());
    let overview = project(&small);
    let first = &overview["tickets"][0];
    assert_eq!(first["thisConversation"], true);
    assert_eq!(overview["tickets"][1]["thisConversation"], false);
    assert_eq!(first["status"], "waiting");
    assert_eq!(first["waiting"]["reason"], "blocked");
    assert_eq!(
        first["agents"][0],
        json!({"id":"agent-0","status":"running","requiresAttention":true})
    );
    assert_eq!(
        first["agents"][1],
        json!({"id":"agent-1","status":"archived"})
    );
    assert_eq!(
        first["latestReport"]["text"].as_str().unwrap().len(),
        MAX_SUMMARY
    );
    assert_eq!(first["latestReport"]["kind"], "blocked");
    assert_eq!(overview["truncated"], false);
    small.all_complete = false;
    let overview = project(&small);
    assert_eq!(overview["tickets"][0]["agents"][1]["status"], Value::Null);
    small.tickets = &NONE;
    assert!(project(&small)["tickets"].is_null());
}

#[test]
fn many_full_tickets_fit_the_bound() {
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let listed = tickets(MAX_TICKETS + 1);
    let mut large = sources(&[], &[], &[], &workspaces, &permissions);
    large.tickets = &listed;
    let overview = project(&large);
    assert!(overview.to_string().len() <= MAX_BYTES);
    assert_eq!(overview["truncated"], true);
    let kept = overview["tickets"].as_array().unwrap();
    assert!(
        !kept.is_empty() && kept.len() <= MAX_TICKETS,
        "{}",
        kept.len()
    );
    assert!(
        kept.iter()
            .all(|ticket| ticket["latestReport"].get("text").is_none())
    );
    assert_eq!(kept[0]["latestReport"]["kind"], "blocked");
}

#[test]
fn clip_respects_character_boundaries() {
    assert_eq!(clip(&json!("你好"), 4), json!("你"));
    assert_eq!(clip(&json!("abc"), 8), json!("abc"));
}
