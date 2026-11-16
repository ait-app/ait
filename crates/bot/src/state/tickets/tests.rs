use chrono::{DateTime, TimeDelta, Utc};
use domain::bot::Turn;
use domain::bot::notification::{Notification, NotificationKind};
use domain::bot::ticket::{TicketDraft, TicketStatus, TicketUpdate, WorkerReportKind};
use domain::{ConversationId, TicketId};
use model::ErrorCode;
use model::bot::Store;
use serde_json::{Value, json};

use super::super::State;
use crate::protocol::decode;
use crate::service::Notified;
use crate::test_support::{Fixture, OTHER_WORKSPACE, WORKSPACE, setup};
use crate::tickets::{AssignTicket, OpenTicket, WorkerReport};

fn active(fixture: &Fixture) -> State {
    let mut state = fixture.state();
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state
}

fn owner(state: &mut State) -> ConversationId {
    let opened = state
        .execute(
            "bot.conversation.open.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    serde_json::from_value(opened["conversation"]["id"].clone()).unwrap()
}

fn request(key: &str, conversation: Option<ConversationId>) -> OpenTicket {
    OpenTicket {
        root: WORKSPACE.into(),
        draft: TicketDraft {
            conversation_id: conversation,
            key: key.into(),
            title: format!("Task {key}"),
            request: "Fix the login crash".into(),
        },
    }
}

/// A new ticket of the active Bot with `agent-1` assigned.
fn assigned(state: &mut State, key: &str, conversation: Option<ConversationId>) -> TicketId {
    let ticket = state
        .open_ticket(request(key, conversation), Utc::now())
        .unwrap()
        .ticket_id;
    state
        .assign_ticket(&assignment(ticket, "agent-1"), Utc::now())
        .unwrap();
    ticket
}

fn assignment(ticket: TicketId, agent: &str) -> AssignTicket {
    AssignTicket {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: agent.into(),
        workspace_id: WORKSPACE.into(),
    }
}

fn report(ticket: TicketId, kind: WorkerReportKind, text: &str) -> WorkerReport {
    WorkerReport {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
        kind,
        text: text.into(),
    }
}

/// The daemon's notification about `agent-1`, labelled with `ticket`.
fn labelled(id: &str, kind: NotificationKind, ticket: TicketId) -> Notification {
    serde_json::from_value(json!({
        "id": id,
        "workspaceId": WORKSPACE,
        "scope": WORKSPACE,
        "kind": kind,
        "agentId": "agent-1",
        "title": "Fix login",
        "summary": "Tests pass",
        "at": Utc::now(),
        "ticketId": ticket,
        "ticketTitle": "spoofed"
    }))
    .unwrap()
}

fn get(state: &State, ticket: TicketId) -> Value {
    let request = decode(json!({"workspaceId":WORKSPACE,"ticketId":ticket})).unwrap();
    state.get_ticket(&request).unwrap()["ticket"].clone()
}

fn list(state: &State, extra: &Value) -> Vec<Value> {
    let mut params = json!({"workspaceId":WORKSPACE});
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    state.list_tickets(&decode(params).unwrap()).unwrap()["tickets"]
        .as_array()
        .unwrap()
        .clone()
}

fn user_update(state: &mut State, ticket: TicketId, status: &str) -> Result<Value, ErrorCode> {
    state.execute(
        "bot.ticket.update.request",
        json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":status}),
    )
}

fn close(status: TicketStatus) -> TicketUpdate {
    TicketUpdate {
        status: Some(status),
        ..TicketUpdate::default()
    }
}

fn later(seconds: i64) -> DateTime<Utc> {
    Utc::now() + TimeDelta::seconds(seconds)
}

/// Every wake turn currently stored.
fn wake_turns(fixture: &Fixture) -> Vec<Turn> {
    fixture
        .store
        .conversations()
        .unwrap()
        .into_iter()
        .flat_map(|conversation| fixture.store.turns(conversation.id).unwrap())
        .filter(|turn| turn.wake.is_some())
        .collect()
}

fn tickets_dir(fixture: &Fixture) -> std::path::PathBuf {
    fixture.root.path().join("bots/tickets")
}

#[test]
fn a_workspace_without_an_active_bot_touches_no_ticket_storage() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let ticket = TicketId::generate();

    assert_eq!(
        state.open_ticket(request("k", None), Utc::now()),
        Err(ErrorCode::BotNotFound)
    );
    assert_eq!(
        state.notify(
            labelled("a", NotificationKind::AgentFinished, ticket),
            Utc::now()
        ),
        Ok(Notified::Ignored)
    );
    assert_eq!(
        state.execute("bot.ticket.list.request", json!({"workspaceId":WORKSPACE})),
        Err(ErrorCode::BotNotFound)
    );
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(
        state.open_ticket(request("k", None), Utc::now()),
        Err(ErrorCode::BotConflict)
    );
    assert!(!tickets_dir(&fixture).exists());
}

#[test]
fn opening_with_the_same_key_returns_the_same_ticket_and_a_different_one_conflicts() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);

    let first = state
        .open_ticket(request("k", Some(conversation)), Utc::now())
        .unwrap();
    let again = state
        .open_ticket(request("k", Some(conversation)), Utc::now())
        .unwrap();
    let mut changed = request("k", Some(conversation));
    changed.draft.request = "Something else".into();
    let mut foreign = request("other", Some(ConversationId::generate()));
    foreign.draft.title = "Other".into();

    assert!(first.created);
    assert_eq!((again.ticket_id, again.created), (first.ticket_id, false));
    assert_eq!(
        state.open_ticket(changed, Utc::now()),
        Err(ErrorCode::IdempotencyConflict)
    );
    assert_eq!(
        state.open_ticket(foreign, Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );
    let mut blank = request("blank", None);
    blank.draft.title = " ".into();
    assert_eq!(
        state.open_ticket(blank, Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(list(&state, &json!({})).len(), 1);
    let ticket = get(&state, first.ticket_id);
    assert_eq!(ticket["conversationId"], json!(conversation));
    assert_eq!(ticket["request"], "Fix the login crash");
    assert!(ticket.get("botId").is_none() && ticket.get("key").is_none());
}

#[test]
fn the_open_limit_refuses_opening_and_reopening_until_tickets_are_closed() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let tickets: Vec<TicketId> = (0..32)
        .map(|index| {
            state
                .open_ticket(request(&index.to_string(), None), Utc::now())
                .unwrap()
                .ticket_id
        })
        .collect();

    assert_eq!(
        state.open_ticket(request("33", None), Utc::now()),
        Err(ErrorCode::ResourceExhausted)
    );
    user_update(&mut state, tickets[0], "done").unwrap();
    let opened = state.open_ticket(request("33", None), Utc::now()).unwrap();
    assert!(opened.created);
    assert_eq!(
        user_update(&mut state, tickets[0], "open"),
        Err(ErrorCode::ResourceExhausted)
    );
    assert_eq!(
        state.update_ticket(
            WORKSPACE,
            tickets[0],
            &close(TicketStatus::Open),
            Utc::now()
        ),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn closed_tickets_beyond_the_limit_are_deleted_earliest_first() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let mut closed = Vec::new();
    for index in 0..65 {
        let ticket = state
            .open_ticket(request(&index.to_string(), None), Utc::now())
            .unwrap()
            .ticket_id;
        state
            .update_ticket(WORKSPACE, ticket, &close(TicketStatus::Done), later(index))
            .unwrap();
        closed.push(ticket);
    }

    let listed = list(&state, &json!({"includeClosed":true,"limit":128}));

    assert_eq!(listed.len(), 64);
    assert_eq!(listed[0]["id"], json!(closed[64]));
    assert!(listed.iter().all(|ticket| ticket["id"] != json!(closed[0])));
    assert!(
        fixture
            .store
            .ticket(state.current(WORKSPACE).unwrap().id, closed[0])
            .is_err()
    );
    assert!(list(&state, &json!({})).is_empty());
}

#[test]
fn assignments_are_idempotent_bounded_and_validated() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);

    state
        .assign_ticket(&assignment(ticket, "agent-1"), Utc::now())
        .unwrap();
    for index in 2..=16 {
        state
            .assign_ticket(&assignment(ticket, &format!("agent-{index}")), Utc::now())
            .unwrap();
    }

    assert_eq!(
        state.assign_ticket(&assignment(ticket, "agent-17"), Utc::now()),
        Err(ErrorCode::ResourceExhausted)
    );
    assert_eq!(
        state.assign_ticket(&assignment(ticket, ""), Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        state.assign_ticket(&assignment(TicketId::generate(), "a"), Utc::now()),
        Err(ErrorCode::BotNotFound)
    );
    let assignments = get(&state, ticket)["assignments"].clone();
    assert_eq!(assignments.as_array().unwrap().len(), 16);
    assert_eq!(assignments[0]["agentId"], "agent-1");
}

#[test]
fn abandoning_deletes_only_a_ticket_without_assignments() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let kept = assigned(&mut state, "kept", None);
    let failed = state
        .open_ticket(request("failed", None), Utc::now())
        .unwrap()
        .ticket_id;

    state.abandon_ticket(WORKSPACE, kept).unwrap();
    state.abandon_ticket(WORKSPACE, failed).unwrap();

    let listed = list(&state, &json!({}));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], json!(kept));
    assert_eq!(
        state.abandon_ticket(WORKSPACE, failed),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn agent_notifications_report_on_the_ticket_and_move_it_between_open_and_waiting() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);

    state
        .notify(
            labelled("p", NotificationKind::AgentPermission, ticket),
            Utc::now(),
        )
        .unwrap();
    let waiting = get(&state, ticket);
    state
        .notify(
            labelled("p", NotificationKind::AgentPermission, ticket),
            Utc::now(),
        )
        .unwrap();
    state
        .notify(
            labelled("f", NotificationKind::AgentFinished, ticket),
            Utc::now(),
        )
        .unwrap();
    let open = get(&state, ticket);

    assert_eq!(waiting["status"], "waiting");
    assert_eq!(waiting["waiting"]["reason"], "permission");
    assert_eq!(open["status"], "open");
    let reports: Vec<_> = open["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|report| (report["kind"].clone(), report["status"].clone()))
        .collect();
    assert_eq!(
        reports,
        [
            (json!("agent_permission"), json!("waiting")),
            (json!("agent_finished"), json!("open"))
        ]
    );
    assert_eq!(open["reports"][1]["text"], "Tests pass");
    let log = &fixture.store.notification_logs().unwrap()[0];
    assert!(log.entries.iter().all(|entry| {
        entry.notification.ticket_id == Some(ticket)
            && entry.notification.ticket_title.as_deref() == Some("Task k")
    }));
    let summary = &list(&state, &json!({}))[0];
    assert_eq!(summary["latestReport"]["kind"], "agent_finished");
    assert!(summary.get("reports").is_none() && summary.get("request").is_none());
}

#[test]
fn a_ticket_the_bot_does_not_own_is_dropped_from_the_notification() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    state
        .execute("bot.setup.request", setup(OTHER_WORKSPACE))
        .unwrap();
    let mut other = request("k", None);
    other.root = OTHER_WORKSPACE.into();
    let foreign = state.open_ticket(other, Utc::now()).unwrap().ticket_id;

    state
        .notify(
            labelled("a", NotificationKind::AgentFinished, foreign),
            Utc::now(),
        )
        .unwrap();

    let logs = fixture.store.notification_logs().unwrap();
    let entry = &logs[0].entries[0].notification;
    assert_eq!(
        (entry.ticket_id, entry.ticket_title.as_deref()),
        (None, None)
    );
    assert_eq!(
        state.execute(
            "bot.ticket.get.request",
            json!({"workspaceId":WORKSPACE,"ticketId":foreign})
        ),
        Err(ErrorCode::BotNotFound)
    );
    let mut foreign_report = report(foreign, WorkerReportKind::Done, "x");
    foreign_report.root = WORKSPACE.into();
    assert_eq!(
        state.report_progress(&foreign_report, Utc::now()),
        Err(ErrorCode::BotNotFound)
    );
}

#[test]
fn a_blocked_report_wakes_the_tickets_conversation_and_names_the_ticket() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));

    let status = state
        .report_progress(
            &report(ticket, WorkerReportKind::Blocked, "Which database?"),
            Utc::now(),
        )
        .unwrap();
    state.wake(later(60));

    assert_eq!(status, TicketStatus::Waiting);
    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].conversation_id, conversation);
    let woke = &turns[0].wake.as_ref().unwrap().notifications[0];
    assert_eq!(woke.kind, NotificationKind::TicketBlocked);
    assert_eq!(woke.id, format!("report:{ticket}:1"));
    assert_eq!(woke.ticket_id, Some(ticket));
    assert_eq!(woke.ticket_title.as_deref(), Some("Task k"));
    assert_eq!(woke.summary.as_deref(), Some("Which database?"));
    assert!(
        turns[0]
            .text
            .contains(&format!("[agent agent-1] [ticket {ticket} \"Task k\"]"))
    );
    assert_eq!(get(&state, ticket)["waiting"]["reason"], "blocked");
}

#[test]
fn progress_never_wakes_and_done_or_failed_wake_without_closing() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);

    state
        .report_progress(
            &report(ticket, WorkerReportKind::Progress, "half way"),
            Utc::now(),
        )
        .unwrap();
    state.wake(later(60));
    let after_progress = wake_turns(&fixture).len();
    let logged_after_progress = fixture.store.notification_logs().unwrap().len();
    let status = state
        .report_progress(
            &report(ticket, WorkerReportKind::Done, "merged"),
            Utc::now(),
        )
        .unwrap();
    state.wake(later(60));

    assert_eq!((after_progress, logged_after_progress), (0, 0));
    assert_eq!(status, TicketStatus::Open);
    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].wake.as_ref().unwrap().notifications[0].kind,
        NotificationKind::TicketDone
    );
    let reports = get(&state, ticket)["reports"].clone();
    assert_eq!(reports[0]["kind"], "progress");
    assert_eq!(reports[1]["source"], "agent");
}

#[test]
fn reports_on_closed_tickets_are_refused_and_labelled_agents_are_heard_and_assigned() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);
    let mut late = report(ticket, WorkerReportKind::Progress, "joined late");
    late.agent_id = "agent-2".into();

    state.report_progress(&late, Utc::now()).unwrap();
    for index in 3..=16 {
        state
            .assign_ticket(&assignment(ticket, &format!("agent-{index}")), Utc::now())
            .unwrap();
    }
    let mut overflow = report(ticket, WorkerReportKind::Done, "my part is done");
    overflow.agent_id = "agent-17".into();
    let status = state.report_progress(&overflow, Utc::now()).unwrap();

    assert_eq!(status, TicketStatus::Open);
    let stored = get(&state, ticket);
    let assignments = stored["assignments"].as_array().unwrap();
    assert_eq!(assignments.len(), 16);
    assert_eq!(assignments[1]["agentId"], "agent-2");
    assert!(
        assignments
            .iter()
            .all(|entry| entry["agentId"] != "agent-17")
    );
    assert_eq!(stored["reports"][1]["agentId"], "agent-17");
    assert_eq!(
        fixture.store.notification_logs().unwrap()[0].entries.len(),
        1
    );
    user_update(&mut state, ticket, "cancelled").unwrap();
    assert_eq!(
        state.report_progress(&report(ticket, WorkerReportKind::Done, "x"), Utc::now()),
        Err(ErrorCode::BotConflict)
    );
}

#[test]
fn an_unreadable_ticket_does_not_keep_the_bot_from_being_woken() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));
    let bot = state.current(WORKSPACE).unwrap().id;
    std::fs::remove_file(
        tickets_dir(&fixture)
            .join(bot.to_string())
            .join(format!("{ticket}.json")),
    )
    .unwrap();

    let notified = state.notify(
        labelled("f", NotificationKind::AgentFinished, ticket),
        Utc::now(),
    );
    state.wake(later(60));

    assert_eq!(notified, Ok(Notified::Admitted));
    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    let woke = &turns[0].wake.as_ref().unwrap().notifications[0];
    assert_eq!(woke.ticket_id, Some(ticket));
    assert_eq!(woke.ticket_title.as_deref(), Some("Task k"));
}

/// `kind` about `agent-1` of `ticket` raised at `at`, ending a turn that started at `started`.
fn turn_end(
    id: &str,
    kind: NotificationKind,
    ticket: TicketId,
    started: Option<DateTime<Utc>>,
    at: DateTime<Utc>,
) -> Notification {
    let mut notification = labelled(id, kind, ticket);
    notification.turn_started_at = started;
    notification.at = at;
    notification
}

/// The wake state of the logged notification `id`.
fn wake_of(fixture: &Fixture, id: &str) -> Value {
    let log = &fixture.store.notification_logs().unwrap()[0];
    let entry = log
        .entries
        .iter()
        .find(|entry| entry.notification.id == id)
        .unwrap();
    serde_json::to_value(entry.wake).unwrap()
}

#[test]
fn a_turn_ending_after_its_own_outcome_report_is_recorded_without_waking_again() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));
    let started = later(-60);
    state
        .report_progress(
            &report(ticket, WorkerReportKind::Done, "merged"),
            later(-30),
        )
        .unwrap();

    state
        .notify(
            turn_end(
                "end-1",
                NotificationKind::AgentFinished,
                ticket,
                Some(started),
                later(-20),
            ),
            later(-20),
        )
        .unwrap();
    state.wake(later(60));

    let turns = wake_turns(&fixture);
    assert_eq!(turns.len(), 1);
    let woke: Vec<_> = turns[0]
        .wake
        .as_ref()
        .unwrap()
        .notifications
        .iter()
        .map(|notification| notification.id.clone())
        .collect();
    assert_eq!(woke, [format!("report:{ticket}:1")]);
    assert_eq!(
        wake_of(&fixture, "end-1"),
        json!({"state":"skipped","reason":"reported"})
    );
    assert_eq!(get(&state, ticket)["reports"][1]["kind"], "agent_finished");
}

#[test]
fn only_an_outcome_report_of_the_same_turn_spares_a_wake_and_never_for_permissions() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));
    state
        .report_progress(
            &report(ticket, WorkerReportKind::Done, "merged"),
            later(-300),
        )
        .unwrap();
    state
        .report_progress(
            &report(ticket, WorkerReportKind::Progress, "follow-up started"),
            later(-100),
        )
        .unwrap();
    let ends = [
        // A later turn, prompted after the report.
        turn_end(
            "later-turn",
            NotificationKind::AgentFinished,
            ticket,
            Some(later(-200)),
            later(-50),
        ),
        // A turn end whose start is unknown.
        turn_end(
            "unknown-start",
            NotificationKind::AgentFailed,
            ticket,
            None,
            later(-40),
        ),
        // A permission request is never spared.
        turn_end(
            "permission",
            NotificationKind::AgentPermission,
            ticket,
            Some(later(-400)),
            later(-30),
        ),
    ];

    for end in ends {
        let at = end.at;
        state.notify(end, at).unwrap();
    }

    for id in ["later-turn", "unknown-start", "permission"] {
        assert_eq!(wake_of(&fixture, id), json!({"state":"pending"}), "{id}");
    }
}

#[test]
fn a_later_turn_or_a_report_the_turn_moved_past_still_wakes_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));
    // The daemon dates every turn below from the same prompt: the provider opened the later
    // turns by itself.
    let started = Some(later(-400));
    let worker = |state: &mut State, kind, text: &str, at| {
        state
            .report_progress(&report(ticket, kind, text), at)
            .unwrap();
    };
    let end = |state: &mut State, id: &str, kind, at| {
        state
            .notify(turn_end(id, kind, ticket, started, at), at)
            .unwrap();
    };

    worker(&mut state, WorkerReportKind::Done, "merged", later(-300));
    end(
        &mut state,
        "first",
        NotificationKind::AgentFinished,
        later(-250),
    );
    end(
        &mut state,
        "autonomous",
        NotificationKind::AgentFinished,
        later(-100),
    );
    worker(
        &mut state,
        WorkerReportKind::Blocked,
        "need the key",
        later(-90),
    );
    worker(
        &mut state,
        WorkerReportKind::Progress,
        "found it",
        later(-80),
    );
    end(
        &mut state,
        "recovered",
        NotificationKind::AgentFinished,
        later(-70),
    );
    worker(&mut state, WorkerReportKind::Done, "shipped", later(-60));
    end(
        &mut state,
        "crashed",
        NotificationKind::AgentFailed,
        later(-50),
    );

    assert_eq!(
        wake_of(&fixture, "first"),
        json!({"state":"skipped","reason":"reported"})
    );
    for id in ["autonomous", "recovered", "crashed"] {
        assert_eq!(wake_of(&fixture, id), json!({"state":"pending"}), "{id}");
    }
}

#[test]
fn ait_closes_a_finished_ticket_and_reopens_one_given_more_work() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let finished = assigned(&mut state, "finished", None);
    state
        .report_progress(
            &report(finished, WorkerReportKind::Done, "merged"),
            Utc::now(),
        )
        .unwrap();
    let waiting = assigned(&mut state, "waiting", None);
    state
        .report_progress(
            &report(waiting, WorkerReportKind::Blocked, "stuck"),
            Utc::now(),
        )
        .unwrap();

    let closed = state.close_finished_ticket(WORKSPACE, finished, Utc::now());
    let left = state.close_finished_ticket(WORKSPACE, waiting, Utc::now());

    assert_eq!(closed, Ok(Some(TicketStatus::Done)));
    assert_eq!(left, Ok(Some(TicketStatus::Cancelled)));
    let stored = get(&state, finished);
    assert_eq!(stored["status"], "done");
    assert!(
        stored["outcome"]
            .as_str()
            .unwrap()
            .contains("automatically")
    );
    let last = stored["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        (last["source"].clone(), last["kind"].clone()),
        (json!("system"), json!("status"))
    );
    let cancelled = get(&state, waiting);
    assert_eq!(cancelled["status"], "cancelled");
    let outcome = cancelled["outcome"].as_str().unwrap();
    assert!(outcome.contains("blocked"), "{outcome}");
    assert!(outcome.contains("archived"), "{outcome}");

    assert_eq!(
        state.reopen_ticket(WORKSPACE, finished, "agent-1", Utc::now()),
        Ok(true)
    );
    assert_eq!(
        state.reopen_ticket(WORKSPACE, finished, "agent-1", Utc::now()),
        Ok(false)
    );
    let reopened = get(&state, finished);
    assert_eq!(reopened["status"], "open");
    let last = reopened["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["source"], "system");
    assert!(last["text"].as_str().unwrap().contains("agent-1"), "{last}");
    state
        .report_progress(
            &report(finished, WorkerReportKind::Done, "again"),
            Utc::now(),
        )
        .unwrap();

    // Reopening respects the open ticket limit; pausing stops both.
    user_update(&mut state, finished, "done").unwrap();
    for index in 0..32 {
        state
            .open_ticket(request(&format!("fill-{index}"), None), Utc::now())
            .unwrap();
    }
    assert_eq!(
        state.reopen_ticket(WORKSPACE, finished, "agent-1", Utc::now()),
        Err(ErrorCode::ResourceExhausted)
    );
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(
        state.close_finished_ticket(WORKSPACE, waiting, Utc::now()),
        Err(ErrorCode::BotConflict)
    );
}

#[test]
fn the_bot_and_the_user_update_tickets_through_their_own_entry_points() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);
    let update = TicketUpdate {
        title: Some("Fix login crash".into()),
        note: Some("Asked for a review".into()),
        ..TicketUpdate::default()
    };

    let noted = state
        .update_ticket(WORKSPACE, ticket, &update, Utc::now())
        .unwrap();
    let closed = state
        .execute(
            "bot.ticket.update.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":"done","outcome":"Merged #12"}),
        )
        .unwrap();
    let repeated = user_update(&mut state, ticket, "done").unwrap();

    assert_eq!(noted["changed"], true);
    assert_eq!(noted["ticket"]["title"], "Fix login crash");
    assert_eq!(closed["ticket"]["status"], "done");
    assert_eq!(closed["ticket"]["outcome"], "Merged #12");
    let last = closed["ticket"]["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        (last["source"].clone(), last["kind"].clone()),
        (json!("user"), json!("status"))
    );
    assert_eq!(repeated["changed"], false);
    assert_eq!(
        user_update(&mut state, ticket, "failed"),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        user_update(&mut state, ticket, "waiting"),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        state.execute(
            "bot.ticket.update.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":"done","note":"x"}),
        ),
        Err(ErrorCode::InvalidMessage)
    );
    assert_eq!(
        state.update_ticket(WORKSPACE, ticket, &TicketUpdate::default(), Utc::now()),
        Err(ErrorCode::InvalidMessage)
    );
    let reopened = user_update(&mut state, ticket, "open").unwrap();
    assert_eq!(reopened["ticket"]["status"], "open");
    assert_eq!(reopened["ticket"]["outcome"], Value::Null);
}

#[test]
fn open_tickets_survive_a_restart_with_every_report() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);
    for text in ["one", "two", "three"] {
        state
            .report_progress(
                &report(ticket, WorkerReportKind::Progress, text),
                Utc::now(),
            )
            .unwrap();
    }
    drop(state);

    let mut restarted = fixture.state();

    let reloaded = get(&restarted, ticket);
    assert_eq!(reloaded["status"], "open");
    assert_eq!(reloaded["reports"].as_array().unwrap().len(), 3);
    assert_eq!(
        list(&restarted, &json!({}))[0]["latestReport"]["text"],
        "three"
    );
    assert_eq!(
        restarted
            .open_ticket(request("k", None), Utc::now())
            .unwrap()
            .ticket_id,
        ticket
    );
    restarted
        .report_progress(
            &report(ticket, WorkerReportKind::Blocked, "four"),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(get(&restarted, ticket)["reports"][3]["sequence"], 4);
}

#[test]
fn pausing_keeps_tickets_and_retiring_or_archiving_removes_them() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);
    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert_eq!(list(&state, &json!({})).len(), 1);
    assert_eq!(
        state.report_progress(&report(ticket, WorkerReportKind::Done, "x"), Utc::now()),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        state.update_ticket(WORKSPACE, ticket, &close(TicketStatus::Done), Utc::now()),
        Err(ErrorCode::BotConflict)
    );
    assert_eq!(
        user_update(&mut state, ticket, "cancelled").unwrap()["changed"],
        true
    );
    let bot = state.current(WORKSPACE).unwrap().id;
    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert!(!tickets_dir(&fixture).join(bot.to_string()).exists());
    state
        .execute("bot.setup.request", setup(WORKSPACE))
        .unwrap();
    assert!(list(&state, &json!({})).is_empty());

    assigned(&mut state, "archived", None);
    let archived = state.current(WORKSPACE).unwrap().id;
    state.retire(&[WORKSPACE.to_owned()]).unwrap();
    assert!(!tickets_dir(&fixture).join(archived.to_string()).exists());
}

#[test]
fn tickets_a_crash_left_behind_for_a_retired_bot_are_deleted_on_load() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = assigned(&mut state, "k", None);
    let bot = state.current(WORKSPACE).unwrap().id;
    let stored = fixture.store.ticket(bot, ticket).unwrap();
    state
        .execute("bot.remove.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    fixture.store.save_ticket(&stored).unwrap();

    drop(fixture.state());

    assert!(!tickets_dir(&fixture).join(bot.to_string()).exists());
}

#[test]
fn a_store_written_without_tickets_still_loads() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let bot = state.current(WORKSPACE).unwrap().id;
    drop(state);
    let log = json!({"botId":bot,"nextSequence":2,"entries":[{"sequence":1,
        "admittedAt":"2026-11-12T08:00:00Z","wake":{"state":"logged"},
        "notification":{"id":"agent:a:1","workspaceId":WORKSPACE,"scope":null,
            "kind":"agent_finished","agentId":"a","scheduleId":null,"runId":null,
            "terminalId":null,"title":"Fix login","summary":null,
            "at":"2026-11-12T08:00:00Z","conversationId":conversation}}]});
    let directory = fixture.root.path().join("bots/notifications");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{bot}.json")),
        serde_json::to_vec(&log).unwrap(),
    )
    .unwrap();

    let mut state = fixture.state();

    assert!(list(&state, &json!({})).is_empty());
    let listed = state
        .execute(
            "bot.notifications.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap();
    assert_eq!(
        listed["notifications"][0]["notification"]["id"],
        "agent:a:1"
    );
    assert!(
        state
            .open_ticket(request("k", None), Utc::now())
            .unwrap()
            .created
    );
}

#[test]
fn a_worker_report_the_conversation_already_read_does_not_wake_it() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let conversation = owner(&mut state);
    let ticket = assigned(&mut state, "k", Some(conversation));
    state
        .send(decode(
            json!({"workspaceId":WORKSPACE,"conversationId":conversation,"messageId":"m1","text":"status?"}),
        ).unwrap())
        .unwrap();
    let run = state.ready().unwrap().remove(0);
    state
        .report_progress(
            &report(ticket, WorkerReportKind::Done, "merged"),
            Utc::now(),
        )
        .unwrap();

    // The running turn's `wait_for_agent` saw agent-1 settled after its report.
    state.observe(conversation, "agent-1", later(1), std::time::Instant::now());
    state
        .progress(
            run.turn.id,
            domain::bot::TurnStatus::Completed,
            Some("merged".into()),
            None,
        )
        .unwrap();
    state.finished(conversation);
    state.wake(later(60));

    assert!(wake_turns(&fixture).is_empty());
    let log = &fixture.store.notification_logs().unwrap()[0];
    assert_eq!(
        serde_json::to_value(log.entries[0].wake).unwrap(),
        json!({"state":"skipped","reason":"observed"})
    );
}
