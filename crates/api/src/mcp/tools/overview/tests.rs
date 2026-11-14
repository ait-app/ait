use std::collections::{BTreeMap, BTreeSet};

use std::sync::LazyLock;

use bot::recall::Viewer;
use domain::bot::{BOT_CONVERSATION_LABEL, BOT_TICKET_LABEL};
use domain::{ConversationId, TicketId};

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
        "updatedAt":"2026-11-06T00:00:00Z"})
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
        own_complete: true,
        all,
        all_complete: true,
        schedules,
        workspaces,
        memory: &MEMORY,
        notifications: &NONE,
        tickets: &NONE,
        sight: None,
        permissions,
    }
}

fn sight(viewer: Viewer) -> Sight {
    Sight {
        viewer,
        tickets: BTreeMap::new(),
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
    // Dropped entries say where each list reads whole.
    assert_eq!(overview["more"]["tickets"]["read"], "tickets");
    assert_eq!(overview["more"]["agents"]["tool"], "list_agents");
}

#[test]
fn small_overview_lists_its_agents_and_is_not_truncated() {
    let own = [agent(0, "wks_root", true), agent(1, "wks_root", true)];
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let overview = project(&sources(&own, &own, &[], &workspaces, &permissions));
    assert_eq!(overview["truncated"], false);
    assert!(overview.get("more").is_none());
    // An Agent's result reads through get_agent_activity; the directory carries no text of it.
    assert_eq!(
        overview["botAgents"][0],
        json!({"id":"agent-0","title":"T".repeat(60),"provider":"codex","status":"idle",
            "requiresAttention":false,"updatedAt":"2026-11-06T00:00:00Z"})
    );
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
            "summary":{"preview":"s".repeat(MAX_SUMMARY),"chars":2048,
                "read":"notification/2"},"read":"notification/2"})
    );
    assert_eq!(overview["notifications"][1]["sequence"], 1);

    let many = listed(MAX_NOTIFICATIONS as u64 + 1);
    // Long Agent titles leave no room for the notifications' summary previews.
    let own: Vec<_> = (0..20)
        .map(|index| {
            let mut agent = agent(index, "wks_root", true);
            agent["title"] = json!("T".repeat(500));
            agent
        })
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
            .all(|entry| { entry["summary"] == json!({"chars":2048,"read":entry["read"]}) })
    );

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
    small.sight = Some(sight(Viewer::Owner(CONVERSATION.parse().unwrap())));
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
        first["latestReport"]["text"]["preview"]
            .as_str()
            .unwrap()
            .len(),
        MAX_SUMMARY
    );
    assert_eq!(
        first["latestReport"]["text"]["read"],
        "ticket/ticket-0/report/3"
    );
    assert_eq!(first["read"], "ticket/ticket-0");
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
    assert!(kept.iter().all(|ticket| {
        ticket["latestReport"]["text"]
            == json!({"chars":2048,"read":format!("ticket/{}/report/3", ticket["id"].as_str().unwrap())})
    }));
    assert_eq!(kept[0]["latestReport"]["kind"], "blocked");
}

#[test]
fn an_external_conversation_sees_only_its_own_notifications_and_report_texts() {
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let mut log = listed(2);
    let own: ConversationId = CONVERSATION.parse().unwrap();
    log["notifications"][0]["notification"]["conversationId"] = json!(own);
    let open = tickets(2);
    let mut external = sources(&[], &[], &[], &workspaces, &permissions);
    external.notifications = &log;
    external.tickets = &open;
    external.sight = Some(sight(Viewer::External(own)));

    let overview = project(&external);

    let notifications = overview["notifications"].as_array().unwrap();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0]["sequence"], 2);
    let tickets = overview["tickets"].as_array().unwrap();
    assert!(tickets[0]["latestReport"].is_object());
    assert!(tickets[0].get("restricted").is_none());
    assert_eq!(tickets[1]["restricted"], true);
    assert!(tickets[1]["latestReport"].is_null());
    assert_eq!(tickets[1]["title"], "T".repeat(256));
    // The owner sees everything.
    external.sight = Some(sight(Viewer::Owner(own)));
    let overview = project(&external);
    assert_eq!(overview["notifications"].as_array().unwrap().len(), 2);
    assert!(overview["tickets"][1]["latestReport"].is_object());
}

/// A Bot Agent titled `title`, with Bot `labels`.
fn bot_agent(id: &str, title: &str, labels: &Value) -> Value {
    let mut agent = agent(0, "wks_root", true);
    agent["id"] = json!(id);
    agent["title"] = json!(title);
    if let (Some(all), Some(extra)) = (agent["labels"].as_object_mut(), labels.as_object()) {
        all.extend(extra.clone());
    }
    agent
}

#[test]
fn an_external_conversation_sees_only_the_agents_of_its_own_tasks() {
    let workspaces = BTreeSet::from(["wks_root".to_owned()]);
    let permissions = BotPermissions::default();
    let (own, alice): (ConversationId, ConversationId) =
        (CONVERSATION.parse().unwrap(), ConversationId::generate());
    let (own_ticket, alices_ticket) = (TicketId::generate(), TicketId::generate());
    let bot_agents = [
        bot_agent(
            "agent-mine",
            "My task",
            &json!({BOT_CONVERSATION_LABEL:own,BOT_TICKET_LABEL:own_ticket}),
        ),
        bot_agent(
            "agent-joined",
            "Owner helping",
            &json!({BOT_TICKET_LABEL:own_ticket}),
        ),
        bot_agent(
            "agent-alice",
            "Q3 layoffs list",
            &json!({BOT_CONVERSATION_LABEL:alice,BOT_TICKET_LABEL:alices_ticket}),
        ),
    ];
    let mut untracked = agent(90, "wks_root", false);
    untracked["title"] = json!("Owner's secret draft");
    untracked["requiresAttention"] = json!(true);
    untracked["attentionReason"] = json!("permission");
    let mut all = bot_agents.to_vec();
    all.push(untracked);
    let open = tickets(2);
    let mut external = sources(&bot_agents, &all, &[], &workspaces, &permissions);
    external.tickets = &open;
    external.sight = Some(Sight {
        viewer: Viewer::External(own),
        tickets: BTreeMap::from([(own_ticket, Some(own)), (alices_ticket, Some(alice))]),
    });

    let overview = project(&external);

    let ids: Vec<&str> = overview["botAgents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|agent| agent["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["agent-mine", "agent-joined"]);
    assert_eq!(overview["botAgents"][0]["title"], "My task");
    // The Agents the Bot did not start are counted, not shown.
    assert_eq!(overview["otherAgents"]["count"], 1);
    assert_eq!(overview["otherAgents"]["attention"], json!([]));
    // Another person's ticket shows how many Agents it has, not which.
    assert!(overview["tickets"][0]["agents"].is_array());
    assert_eq!(overview["tickets"][1]["agents"], 16);
    // It says why it waits, not which Agent made it wait.
    assert_eq!(
        overview["tickets"][1]["waiting"],
        json!({"reason":"blocked","since":"2026-11-13T00:00:00Z"})
    );
    assert_eq!(overview["tickets"][0]["waiting"]["agentId"], "agent-0");
    let text = overview.to_string();
    for hidden in ["secret", "Q3 layoffs", "agent-alice", "agent-90"] {
        assert!(!text.contains(hidden), "{hidden}: {text}");
    }
    assert_eq!(overview["truncated"], false);
    // Bot Agents beyond the page read may be this conversation's, so the overview says so.
    external.own_complete = false;
    assert_eq!(project(&external)["truncated"], true);
    // The owner sees every Agent.
    external.sight = Some(sight(Viewer::Owner(own)));
    let overview = project(&external);
    assert_eq!(overview["botAgents"].as_array().unwrap().len(), 3);
    assert_eq!(overview["otherAgents"]["attention"][0]["id"], "agent-90");
    assert!(overview["tickets"][1]["agents"].is_array());
}

#[test]
fn clip_respects_character_boundaries() {
    assert_eq!(clip(&json!("你好"), 4), json!("你"));
    assert_eq!(clip(&json!("abc"), 8), json!("abc"));
}
