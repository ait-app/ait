use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use domain::bot::notification::Notification;
use domain::bot::ticket::{TicketDraft, TicketStatus, TicketUpdate, WorkerReportKind};
use domain::bot::{
    BOT_CONVERSATION_LABEL, BOT_TICKET_LABEL, Turn, TurnMessage, TurnMessageStatus, TurnStatus,
};
use domain::{ChannelId, ConversationId, TicketId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::super::State;
use crate::channels::Incoming;
use crate::recall::{DETAIL, Origin, PREVIEW, Read, Reader, Viewer};
use crate::test_support::{Fixture, WORKSPACE, setup};
use crate::tickets::{AssignTicket, OpenTicket, WorkerReport};

/// An active Bot with its owner conversation, a Telegram channel and the conversations of two
/// of its senders, `alice` and `bob`, each with one turn.
struct World {
    state: State,
    owner: ConversationId,
    alice: ConversationId,
    bob: ConversationId,
}

fn world(fixture: &Fixture) -> World {
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    let owner = serde_json::from_value(opened["conversation"]["id"].clone()).unwrap();
    let channel = ChannelId::generate();
    let config = json!({"kind":"telegram","tokenRef":"env:AIT_SERVER_CREDENTIAL_TELEGRAM",
        "allowedSenders":["alice","bob"]});
    state
        .execute(
            "bot.channel.create.request",
            json!({"workspaceId":WORKSPACE,"channelId":channel,"config":config,"enabled":true}),
        )
        .unwrap();
    let alice = said(&mut state, channel, "alice", "alice-1");
    let bob = said(&mut state, channel, "bob", "bob-1");
    World {
        state,
        owner,
        alice,
        bob,
    }
}

/// `sender` writes in the Telegram chat; returns its conversation.
fn said(state: &mut State, channel: ChannelId, sender: &str, message: &str) -> ConversationId {
    let accepted = state
        .accept(Incoming {
            channel_id: channel,
            peer: "chat-7".into(),
            sender: sender.into(),
            thread: None,
            message_id: message.into(),
            text: format!("{sender} asks for a fix"),
            reply_to: None,
        })
        .unwrap();
    serde_json::from_value(accepted["turn"]["conversationId"].clone()).unwrap()
}

/// The single turn of `conversation`, completed with the given assistant messages.
fn answered(state: &mut State, conversation: ConversationId, messages: &[&str]) -> Turn {
    let id = *state
        .order
        .range((conversation, 0)..=(conversation, u64::MAX))
        .next()
        .unwrap()
        .1;
    let mut turn = state.turn(id).unwrap();
    turn.status = TurnStatus::Completed;
    turn.messages = messages
        .iter()
        .enumerate()
        .map(|(index, text)| TurnMessage {
            id: format!("m{index}"),
            text: (*text).to_owned(),
            status: TurnMessageStatus::Completed,
        })
        .collect();
    state.commit_turn(&turn).unwrap();
    turn
}

fn ticket(state: &mut State, key: &str, conversation: ConversationId, request: &str) -> TicketId {
    let opened = state
        .open_ticket(
            OpenTicket {
                root: WORKSPACE.into(),
                draft: TicketDraft {
                    conversation_id: Some(conversation),
                    key: key.into(),
                    title: format!("Task {key}"),
                    request: request.into(),
                },
            },
            Utc::now(),
        )
        .unwrap();
    state
        .assign_ticket(
            &AssignTicket {
                root: WORKSPACE.into(),
                ticket_id: opened.ticket_id,
                agent_id: format!("agent-{key}"),
                workspace_id: WORKSPACE.into(),
            },
            Utc::now(),
        )
        .unwrap();
    opened.ticket_id
}

fn report(state: &mut State, ticket: TicketId, agent: &str, kind: WorkerReportKind, text: &str) {
    state
        .report_progress(
            &WorkerReport {
                root: WORKSPACE.into(),
                ticket_id: ticket,
                agent_id: agent.into(),
                workspace_id: WORKSPACE.into(),
                kind,
                text: text.into(),
            },
            Utc::now(),
        )
        .unwrap();
}

fn notification(id: &str, conversation: Option<ConversationId>, summary: &str) -> Notification {
    serde_json::from_value(json!({
        "id": id,
        "workspaceId": WORKSPACE,
        "scope": WORKSPACE,
        "kind": "agent_failed",
        "agentId": "agent-x",
        "title": "Agent failed",
        "summary": summary,
        "at": Utc::now(),
        "conversationId": conversation,
    }))
    .unwrap()
}

fn request(reader: Reader, resource: &str) -> Read {
    Read {
        root: WORKSPACE.into(),
        reader,
        resource: resource.into(),
        offset: None,
        limit: None,
        agent: None,
    }
}

fn read(state: &State, reader: Reader, resource: &str) -> Result<Value, ErrorCode> {
    state.read(&request(reader, resource))
}

fn page(state: &State, reader: Reader, resource: &str, offset: usize, limit: usize) -> Value {
    let mut read = request(reader, resource);
    read.offset = Some(offset);
    read.limit = Some(limit);
    state.read(&read).unwrap()
}

/// Read every page of text resource `resource` and join them.
fn whole(state: &State, reader: Reader, resource: &str) -> String {
    let mut text = String::new();
    let mut offset = 0;
    loop {
        let page = page(state, reader, resource, offset, 1000);
        text.push_str(page["text"].as_str().unwrap());
        match page["nextOffset"].as_u64() {
            Some(next) => offset = usize::try_from(next).unwrap(),
            None => return text,
        }
    }
}

/// Follow the read pointer of spilled field `value` and return the whole text.
fn follow(state: &State, reader: Reader, value: &Value) -> String {
    let pointer = value["read"]
        .as_str()
        .unwrap_or_else(|| panic!("no pointer: {value}"));
    let text = whole(state, reader, pointer);
    assert_eq!(
        text.chars().count(),
        usize::try_from(value["chars"].as_u64().unwrap()).unwrap()
    );
    assert!(text.starts_with(value["preview"].as_str().unwrap()));
    text
}

fn ids(page: &Value) -> Vec<String> {
    page["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_owner_walks_from_tickets_to_their_reports_and_whole_texts() {
    let fixture = Fixture::new();
    let World {
        mut state,
        owner,
        alice,
        ..
    } = world(&fixture);
    let me = Reader::Conversation(owner);
    let long_request = "r".repeat(3000);
    let task = ticket(&mut state, "fix", alice, &long_request);
    report(
        &mut state,
        task,
        "agent-fix",
        WorkerReportKind::Progress,
        "half way",
    );
    let long_report = "d".repeat(2000);
    report(
        &mut state,
        task,
        "agent-fix",
        WorkerReportKind::Done,
        &long_report,
    );
    let done = ticket(&mut state, "old", owner, "old work");
    state
        .update_ticket(
            WORKSPACE,
            done,
            &TicketUpdate {
                status: Some(TicketStatus::Done),
                outcome: Some("shipped".into()),
                ..TicketUpdate::default()
            },
            Utc::now(),
        )
        .unwrap();

    // Open tickets list the card of each, with the newest report and the conversation.
    let tickets = read(&state, me, "tickets").unwrap();
    assert_eq!(tickets["resource"], "tickets");
    assert_eq!(ids(&tickets), [task.to_string()]);
    let card = &tickets["items"][0];
    assert_eq!(card["read"], format!("ticket/{task}"));
    assert_eq!(card["conversation"], format!("conversation/{alice}"));
    assert_eq!(card["thisConversation"], false);
    assert_eq!(card["agents"], 1);
    assert_eq!(card["latestReport"]["kind"], "done");
    assert_eq!(
        follow(&state, me, &card["latestReport"]["text"]),
        long_report
    );
    let closed = read(&state, me, "tickets/closed").unwrap();
    assert_eq!(ids(&closed), [done.to_string()]);
    assert_eq!(
        read(&state, me, &format!("ticket/{done}/outcome")).unwrap()["text"],
        "shipped"
    );

    // The ticket page spills its request and lists reports newest first.
    let full = read(&state, me, &format!("ticket/{task}")).unwrap();
    assert_eq!(full["request"]["chars"], 3000);
    assert_eq!(full["request"]["preview"].as_str().unwrap().len(), DETAIL);
    assert_eq!(follow(&state, me, &full["request"]), long_request);
    assert_eq!(full["assignments"][0]["agentId"], "agent-fix");
    assert_eq!(full["assignments"][0]["read"], "agent/agent-fix");
    assert_eq!(full["assignments"][0]["latestReport"]["kind"], "done");
    assert_eq!(full["reports"]["items"][0]["kind"], "done");
    assert_eq!(full["reports"]["read"], format!("ticket/{task}/reports"));
    let reports = read(&state, me, &format!("ticket/{task}/reports")).unwrap();
    let sequences: Vec<u64> = reports["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["sequence"].as_u64().unwrap())
        .collect();
    assert!(
        sequences.windows(2).all(|pair| pair[0] > pair[1]),
        "{sequences:?}"
    );
    let progress = read(
        &state,
        me,
        &format!("ticket/{task}/report/{}", sequences[1]),
    )
    .unwrap();
    assert_eq!(progress["text"], "half way");
    assert_eq!(progress["kind"], "progress");
}

#[test]
fn the_owner_walks_from_conversations_to_turns_and_their_messages() {
    let fixture = Fixture::new();
    let World {
        mut state,
        owner,
        alice,
        ..
    } = world(&fixture);
    let me = Reader::Conversation(owner);
    ticket(&mut state, "fix", alice, "fix it");
    let turn = answered(&mut state, alice, &["first reply", &"z".repeat(1500)]);

    // Conversations lead to their turns and each turn's messages.
    let conversations = read(&state, me, "conversations").unwrap();
    assert_eq!(conversations["items"].as_array().unwrap().len(), 3);
    let row = conversations["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == alice.to_string())
        .unwrap();
    assert_eq!(row["channel"]["kind"], "telegram");
    assert_eq!(row["label"], "telegram chat-7 / alice");
    assert_eq!(row["openTickets"], 1);
    assert!(row["lastActivityAt"].is_string());
    let history = read(&state, me, &format!("conversation/{alice}")).unwrap();
    let item = &history["turns"]["items"][0];
    assert_eq!(item["id"], turn.id.to_string());
    assert_eq!(item["input"], "alice asks for a fix");
    assert_eq!(item["reply"]["chars"], 1500);
    assert_eq!(item["reply"]["preview"].as_str().unwrap().len(), PREVIEW);
    assert_eq!(follow(&state, me, &item["reply"]), "z".repeat(1500));
    let one = read(&state, me, &format!("turn/{}", turn.id)).unwrap();
    assert_eq!(one["conversation"], format!("conversation/{alice}"));
    assert_eq!(one["messages"]["items"][0]["text"], "first reply");
    assert_eq!(
        one["messages"]["items"][1]["text"]["read"],
        format!("turn/{}/message/1", turn.id)
    );
    assert_eq!(
        read(&state, me, &format!("turn/{}/input", turn.id)).unwrap()["text"],
        "alice asks for a fix"
    );
}

#[test]
fn notifications_point_at_their_ticket_report_and_wake_turn() {
    let fixture = Fixture::new();
    let World {
        mut state, owner, ..
    } = world(&fixture);
    let me = Reader::Conversation(owner);
    let task = ticket(&mut state, "fix", owner, "fix it");
    report(
        &mut state,
        task,
        "agent-fix",
        WorkerReportKind::Blocked,
        &"q".repeat(1500),
    );
    state
        .notify(
            notification("agent:x:1", None, &"s".repeat(700)),
            Utc::now(),
        )
        .unwrap();

    let listed = read(&state, me, "notifications").unwrap();
    let items = listed["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    // The free-form notification id stays off the page; the sequence reads it.
    assert!(items.iter().all(|item| item.get("id").is_none()));
    let failed = &items[0];
    assert_eq!(failed["kind"], "agent_failed");
    assert_eq!(failed["agent"], "agent/agent-x");
    assert_eq!(failed["summary"]["chars"], 700);
    assert_eq!(failed["summary"]["read"], failed["read"]);
    let full = read(&state, me, failed["read"].as_str().unwrap()).unwrap();
    assert_eq!(full["summary"], "s".repeat(700));

    let blocked = read(&state, me, items[1]["read"].as_str().unwrap()).unwrap();
    assert_eq!(blocked["kind"], "ticket_blocked");
    assert_eq!(blocked["ticket"]["read"], format!("ticket/{task}"));
    assert_eq!(blocked["conversation"], format!("conversation/{owner}"));
    // The summary was cut at admission; the report pointer returns the whole report.
    let report = blocked["report"].as_str().unwrap();
    assert_eq!(whole(&state, me, report), "q".repeat(1500));
    assert_eq!(
        read(&state, me, "notification/99"),
        Err(ErrorCode::BotNotFound)
    );
}

/// A world where Alice has a blocked ticket, an answered turn and a notification, the owner
/// has a ticket, and a Workspace event has no conversation.
struct Shared {
    world: World,
    alice_ticket: TicketId,
    owner_ticket: TicketId,
    alice_turn: Turn,
}

fn shared(fixture: &Fixture) -> Shared {
    let mut world = world(fixture);
    let (state, alice) = (&mut world.state, world.alice);
    let alice_ticket = ticket(state, "alice", alice, "alice's secret task");
    report(
        state,
        alice_ticket,
        "agent-alice",
        WorkerReportKind::Blocked,
        "which secret database?",
    );
    let owner_ticket = ticket(state, "owner", world.owner, "owner's task");
    let alice_turn = answered(state, alice, &["alice's private reply"]);
    for (id, conversation, summary) in [
        ("agent:x:1", Some(alice), "alice's failure"),
        ("agent:y:1", None, "workspace event"),
    ] {
        state
            .notify(notification(id, conversation, summary), Utc::now())
            .unwrap();
    }
    Shared {
        world,
        alice_ticket,
        owner_ticket,
        alice_turn,
    }
}

#[test]
fn an_external_conversation_reads_only_itself_and_what_it_started() {
    let fixture = Fixture::new();
    let Shared {
        world:
            World {
                state,
                owner,
                alice,
                bob,
            },
        alice_ticket,
        owner_ticket,
        alice_turn,
    } = shared(&fixture);
    let as_bob = Reader::Conversation(bob);

    let sight = state
        .sight(WORKSPACE, Origin::Conversation(bob))
        .unwrap()
        .unwrap();
    assert_eq!(sight.viewer, Viewer::External(bob));
    // An external viewer gets every ticket's origin, to decide on Agents by their labels.
    assert_eq!(sight.tickets.get(&alice_ticket), Some(&Some(alice)));
    assert_eq!(sight.tickets.get(&owner_ticket), Some(&Some(owner)));
    let owners = state
        .sight(WORKSPACE, Origin::Conversation(owner))
        .unwrap()
        .unwrap();
    assert_eq!(owners.viewer, Viewer::Owner(owner));
    assert!(owners.tickets.is_empty());
    // Bob's conversation list is Bob alone; Alice's conversation and turns are refused.
    assert_eq!(
        ids(&read(&state, as_bob, "conversations").unwrap()),
        [bob.to_string()]
    );
    for refused in [
        format!("conversation/{alice}"),
        format!("turn/{}", alice_turn.id),
        format!("turn/{}/input", alice_turn.id),
        format!("turn/{}/message/0", alice_turn.id),
        format!("ticket/{alice_ticket}/reports"),
        format!("ticket/{alice_ticket}/request"),
    ] {
        assert_eq!(
            read(&state, as_bob, &refused),
            Err(ErrorCode::Forbidden),
            "{refused}"
        );
    }
    // Notification sequences are small counters: another conversation's entry reads as not
    // kept, like a missing one, so Bob cannot count or time Alice's notifications.
    assert_eq!(
        read(&state, as_bob, "notification/1"),
        read(&state, as_bob, "notification/99")
    );
    assert_eq!(
        read(&state, as_bob, "notification/1"),
        Err(ErrorCode::BotNotFound)
    );
    // Other people's tickets are only titles and statuses, as in the overview.
    let tickets = read(&state, as_bob, "tickets").unwrap();
    assert_eq!(tickets["items"].as_array().unwrap().len(), 2);
    for item in tickets["items"].as_array().unwrap() {
        assert_eq!(item["restricted"], true);
        assert!(item.get("latestReport").is_none() && item.get("conversation").is_none());
    }
    let stub = read(&state, as_bob, &format!("ticket/{owner_ticket}")).unwrap();
    assert_eq!(stub["restricted"], true);
    assert_eq!(stub["title"], "Task owner");
    let bob_view = read(&state, as_bob, "notifications").unwrap();
    assert_eq!(bob_view["items"], json!([]));
    let everything = [
        read(&state, as_bob, "tickets").unwrap(),
        read(&state, as_bob, &format!("ticket/{alice_ticket}")).unwrap(),
        read(&state, as_bob, "conversations").unwrap(),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<String>();
    assert!(!everything.contains("secret") && !everything.contains(&alice.to_string()));
    // A waiting stub says why, not which Agent made it wait.
    let waiting = read(&state, as_bob, &format!("ticket/{alice_ticket}")).unwrap();
    assert_eq!(waiting["waiting"]["reason"], "blocked");
    assert!(!everything.contains("agent-alice"), "{everything}");
}

#[test]
fn alice_reads_her_own_work_and_the_owner_reads_everything() {
    let fixture = Fixture::new();
    let Shared {
        world:
            World {
                state,
                owner,
                alice,
                ..
            },
        alice_ticket,
        owner_ticket,
        alice_turn,
    } = shared(&fixture);
    let as_alice = Reader::Conversation(alice);

    // Alice reads her own ticket, turn and notifications, and none of the owner's.
    let own = read(&state, as_alice, &format!("ticket/{alice_ticket}")).unwrap();
    assert_eq!(own["request"], "alice's secret task");
    assert_eq!(own["thisConversation"], true);
    assert_eq!(own["waiting"]["agentId"], "agent-alice");
    assert!(read(&state, as_alice, &format!("turn/{}", alice_turn.id)).is_ok());
    let alice_view = read(&state, as_alice, "notifications").unwrap();
    let kinds: Vec<&str> = alice_view["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["agent_failed", "ticket_blocked"]);
    assert_eq!(alice_view["items"][0]["summary"], "alice's failure");
    assert_eq!(
        read(&state, as_alice, &format!("ticket/{owner_ticket}/reports")),
        Err(ErrorCode::Forbidden)
    );

    // The owner reads Alice's conversation and its turns.
    let me = Reader::Conversation(owner);
    let history = read(&state, me, &format!("conversation/{alice}")).unwrap();
    assert_eq!(
        history["turns"]["items"][0]["reply"],
        "alice's private reply"
    );
    assert_eq!(
        read(&state, me, "notifications").unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn agents_lead_to_their_ticket_and_conversation_within_the_reader_rule() {
    let fixture = Fixture::new();
    let World {
        mut state,
        owner,
        alice,
        bob,
    } = world(&fixture);
    let task = ticket(&mut state, "fix", alice, "fix it");
    report(
        &mut state,
        task,
        "agent-fix",
        WorkerReportKind::Done,
        "fixed",
    );
    let record = json!({"id":"agent-fix","title":"Fixer","status":"idle",
        "requiresAttention":false,"workspaceId":WORKSPACE,"updatedAt":"2026-11-14T00:00:00Z",
        "archivedAt":null,"labels":{BOT_TICKET_LABEL:task,BOT_CONVERSATION_LABEL:alice}});
    let agent = |reader| {
        let mut read = request(reader, "agent/agent-fix");
        read.agent = Some(record.clone());
        state.read(&read).unwrap()
    };

    let seen = agent(Reader::Conversation(owner));
    assert_eq!(seen["ticket"]["read"], format!("ticket/{task}"));
    assert_eq!(seen["conversation"], format!("conversation/{alice}"));
    assert_eq!(seen["latestReport"]["text"], "fixed");
    assert_eq!(seen["transcript"], "get_agent_activity");
    // Alice reads the Agent of her own task in full.
    let own = agent(Reader::Conversation(alice));
    assert_eq!(own["title"], "Fixer");
    assert_eq!(own["latestReport"]["text"], "fixed");
    // Bob may not read it at all: not its title or report.
    let mut as_bob = request(Reader::Conversation(bob), "agent/agent-fix");
    as_bob.agent = Some(record.clone());
    assert_eq!(state.read(&as_bob), Err(ErrorCode::Forbidden));
    // Nor an Agent the Bot did not start, such as the owner's own work.
    let unlabelled = json!({"id":"agent-mine","title":"Owner's own","status":"idle",
        "workspaceId":WORKSPACE,"labels":{}});
    let mut owners = request(Reader::Conversation(bob), "agent/agent-mine");
    owners.agent = Some(unlabelled.clone());
    assert_eq!(state.read(&owners), Err(ErrorCode::Forbidden));
    owners.reader = Reader::Conversation(owner);
    assert_eq!(state.read(&owners).unwrap()["title"], "Owner's own");
    // An Agent Bob's conversation started is his, even on a ticket kept without a conversation.
    let mut bobs = request(Reader::Conversation(bob), "agent/agent-bob");
    bobs.agent = Some(json!({"id":"agent-bob","title":"Bob's","status":"idle",
        "workspaceId":WORKSPACE,"labels":{BOT_CONVERSATION_LABEL:bob}}));
    assert_eq!(state.read(&bobs).unwrap()["title"], "Bob's");
    // A ticket of another conversation decides over the Agent's own label: an Agent Bob once
    // joined to Alice's ticket reads that ticket, so Bob may not reach it.
    bobs.agent = Some(json!({"id":"agent-bob","title":"Bob's","status":"idle",
        "workspaceId":WORKSPACE,"labels":{BOT_CONVERSATION_LABEL:bob,BOT_TICKET_LABEL:task}}));
    assert_eq!(state.read(&bobs), Err(ErrorCode::Forbidden));
    // Without the caller's record the owner is told nothing is kept. Bob is told the same as
    // for someone else's Agent, also when the record is of another id (a resolved prefix or
    // title), so his answer does not confirm which ids exist.
    assert_eq!(
        read(&state, Reader::Conversation(owner), "agent/agent-fix"),
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        read(&state, Reader::Conversation(bob), "agent/no-such-agent"),
        Err(ErrorCode::Forbidden)
    );
    bobs.resource = "agent/agent".into();
    assert_eq!(state.read(&bobs), Err(ErrorCode::Forbidden));
}

#[test]
fn an_agent_sees_as_the_conversation_its_work_came_from() {
    let fixture = Fixture::new();
    let World {
        mut state,
        owner,
        alice,
        bob,
    } = world(&fixture);
    let alices = ticket(&mut state, "alice", alice, "fix it");
    let owners = ticket(&mut state, "owner", owner, "own work");
    let unknown = TicketId::generate();
    let viewer = |ticket: Option<TicketId>, conversation: Option<ConversationId>| {
        state
            .sight(
                WORKSPACE,
                Origin::Agent {
                    ticket,
                    conversation,
                },
            )
            .unwrap()
            .map(|sight| sight.viewer)
    };

    // The ticket decides over the Agent's own label.
    assert_eq!(
        viewer(Some(alices), Some(bob)),
        Some(Viewer::External(alice))
    );
    assert_eq!(viewer(Some(owners), Some(bob)), Some(Viewer::Owner(owner)));
    // Without a ticket the Bot keeps, the conversation label decides.
    assert_eq!(
        viewer(Some(unknown), Some(bob)),
        Some(Viewer::External(bob))
    );
    assert_eq!(viewer(None, Some(bob)), Some(Viewer::External(bob)));
    assert_eq!(viewer(None, Some(owner)), Some(Viewer::Owner(owner)));
    // An Agent whose work came from no conversation is not restricted.
    assert_eq!(viewer(Some(unknown), None), None);
    assert_eq!(viewer(None, None), None);
    // A restricted Agent decides on other Agents with every ticket's origin.
    let sight = state
        .sight(
            WORKSPACE,
            Origin::Agent {
                ticket: Some(alices),
                conversation: None,
            },
        )
        .unwrap()
        .unwrap();
    assert_eq!(sight.tickets.get(&owners), Some(&Some(owner)));
}

#[test]
fn a_worker_reads_only_its_own_ticket() {
    let fixture = Fixture::new();
    let World {
        mut state, owner, ..
    } = world(&fixture);
    let own = ticket(&mut state, "own", owner, &"w".repeat(2000));
    let other = ticket(&mut state, "other", owner, "other work");
    state
        .assign_ticket(
            &AssignTicket {
                root: WORKSPACE.into(),
                ticket_id: own,
                agent_id: "agent-peer".into(),
                workspace_id: WORKSPACE.into(),
            },
            Utc::now(),
        )
        .unwrap();
    report(
        &mut state,
        own,
        "agent-peer",
        WorkerReportKind::Progress,
        "peer found the bug",
    );
    let worker = Reader::Worker(own);

    let page = read(&state, worker, "ticket").unwrap();
    assert_eq!(page["resource"], "ticket");
    assert_eq!(page["id"], own.to_string());
    assert_eq!(page["request"]["read"], "ticket/request");
    assert_eq!(follow(&state, worker, &page["request"]), "w".repeat(2000));
    let peer = &page["assignments"][1];
    assert_eq!(peer["agentId"], "agent-peer");
    assert_eq!(peer["latestReport"]["text"], "peer found the bug");
    assert!(peer.get("read").is_none());
    assert_eq!(page["reports"]["read"], "ticket/reports");
    // No other ticket, conversation or notification id is on its pages.
    let text = page.to_string();
    assert!(!text.contains(&owner.to_string()) && !text.contains(&other.to_string()));
    assert!(page.get("conversation").is_none() && page.get("thisConversation").is_none());
    let reports = read(&state, worker, "ticket/reports").unwrap();
    let newest = reports["items"][0]["sequence"].as_u64().unwrap();
    assert_eq!(
        read(&state, worker, &format!("ticket/report/{newest}")).unwrap()["text"],
        "peer found the bug"
    );
    for refused in [
        format!("ticket/{other}"),
        format!("ticket/{own}"),
        "tickets".to_owned(),
        "conversations".to_owned(),
        format!("conversation/{owner}"),
        "notifications".to_owned(),
        "agent/agent-peer".to_owned(),
    ] {
        assert_eq!(
            read(&state, worker, &refused),
            Err(ErrorCode::InvalidMessage),
            "{refused}"
        );
    }
    // A worker whose ticket the Bot no longer has reads nothing.
    let gone = Reader::Worker(TicketId::generate());
    assert_eq!(read(&state, gone, "ticket"), Err(ErrorCode::BotNotFound));
}

/// Every file under `root` with its contents.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.insert(path.display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    found
}

#[test]
fn reads_write_nothing_and_leave_unseen_notifications_unseen() {
    let fixture = Fixture::new();
    let World {
        mut state,
        owner,
        alice,
        ..
    } = world(&fixture);
    let task = ticket(&mut state, "fix", owner, "fix it");
    let turn = answered(&mut state, alice, &["reply"]);
    state
        .notify(notification("agent:x:1", Some(owner), "failed"), Utc::now())
        .unwrap();
    let unseen = json!({"workspaceId":WORKSPACE,"unseenBy":owner});
    let before = state
        .execute("bot.notifications.list.request", unseen.clone())
        .unwrap();
    let stored = files(&fixture.root.path().join("bots"));
    let me = Reader::Conversation(owner);

    for resource in [
        "tickets".to_owned(),
        format!("ticket/{task}"),
        format!("ticket/{task}/reports"),
        "conversations".to_owned(),
        format!("conversation/{alice}"),
        format!("turn/{}", turn.id),
        "notifications".to_owned(),
        "notification/1".to_owned(),
    ] {
        read(&state, me, &resource).unwrap();
    }
    read(&state, Reader::Worker(task), "ticket").unwrap();

    assert_eq!(files(&fixture.root.path().join("bots")), stored);
    assert_eq!(
        state
            .execute("bot.notifications.list.request", unseen)
            .unwrap(),
        before
    );
}

#[test]
fn unknown_resources_and_pages_beyond_the_bounds_are_refused() {
    let fixture = Fixture::new();
    let World { state, owner, .. } = world(&fixture);
    let me = Reader::Conversation(owner);
    for resource in [
        "",
        "ticket",
        "ticket/not-a-ticket",
        "tickets/open",
        "conversation/x",
        "turn",
        "notification/first",
        "agent/",
        "memory",
    ] {
        assert_eq!(
            read(&state, me, resource),
            Err(ErrorCode::InvalidMessage),
            "{resource}"
        );
    }
    let mut oversized = request(me, "tickets");
    oversized.limit = Some(51);
    assert_eq!(state.read(&oversized), Err(ErrorCode::InvalidMessage));
    assert_eq!(
        read(&state, me, &format!("ticket/{}", TicketId::generate())),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn a_retired_bot_is_not_readable() {
    let fixture = Fixture::new();
    let World {
        mut state, owner, ..
    } = world(&fixture);
    let task = ticket(&mut state, "fix", owner, "fix it");
    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    let me = Reader::Conversation(owner);
    for resource in [
        "tickets".to_owned(),
        format!("ticket/{task}"),
        "conversations".to_owned(),
    ] {
        assert_eq!(read(&state, me, &resource), Err(ErrorCode::BotNotFound));
    }
    assert_eq!(
        state.sight(WORKSPACE, Origin::Conversation(owner)),
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        read(&state, Reader::Worker(task), "ticket"),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn turn_and_list_pages_continue_from_their_next_offset() {
    let fixture = Fixture::new();
    let World {
        mut state, owner, ..
    } = world(&fixture);
    let task = ticket(&mut state, "fix", owner, "fix it");
    for index in 0..5 {
        report(
            &mut state,
            task,
            "agent-fix",
            WorkerReportKind::Progress,
            &format!("step {index}"),
        );
    }
    let me = Reader::Conversation(owner);
    let resource = format!("ticket/{task}/reports");
    let first = page(&state, me, &resource, 0, 2);
    assert_eq!(first["items"][0]["text"], "step 4");
    assert_eq!(first["nextOffset"], 2);
    let last = page(&state, me, &resource, 4, 2);
    assert_eq!(last["items"][0]["text"], "step 0");
    assert!(last["nextOffset"].is_null());
    let texts: Vec<Value> = (0..3)
        .flat_map(|index| {
            page(&state, me, &resource, index * 2, 2)["items"]
                .as_array()
                .unwrap()
                .clone()
        })
        .map(|item| item["text"].clone())
        .collect();
    assert_eq!(
        texts,
        ["step 4", "step 3", "step 2", "step 1", "step 0"].map(Value::from)
    );
}

#[test]
fn the_conversation_list_names_each_channel_kind_and_last_activity() {
    let fixture = Fixture::new();
    let World {
        mut state, alice, ..
    } = world(&fixture);
    let listed = state
        .execute(
            "bot.conversation.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    let rows = listed["conversations"].as_array().unwrap();
    let kinds: Vec<&str> = rows
        .iter()
        .map(|row| row["channelKind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds.iter().filter(|kind| **kind == "telegram").count(), 2);
    assert!(kinds.contains(&"builtin"));
    let alice_row = rows
        .iter()
        .find(|row| row["id"] == alice.to_string())
        .unwrap();
    let turn = answered(&mut state, alice, &["done"]);
    assert_eq!(alice_row["lastActivityAt"], turn.created_at);
    // The owner conversation has no turn yet.
    let owner_row = rows
        .iter()
        .find(|row| row["channelKind"] == "builtin")
        .unwrap();
    assert!(owner_row["lastActivityAt"].is_null());
}
