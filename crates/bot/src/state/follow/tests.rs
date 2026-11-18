//! Following pull requests and answering blocked workers on the Bot state (ADR-287).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use domain::bot::ticket::{TicketDraft, TicketStatus, TicketUpdate, WorkerReportKind};
use domain::forge::{AuthorAccess, PullItem, PullItemKind, PullSnapshot, PullState};
use domain::{ConversationId, TicketId};
use model::ErrorCode;
use serde_json::{Value, json};

use super::super::State;
use crate::follow::{FOLLOW_LIMIT, PullRead};
use crate::test_support::{Fixture, WORKSPACE, setup};
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

/// A ticket from the owner conversation with `agent-1` assigned.
fn ticket(state: &mut State, key: &str) -> TicketId {
    let conversation = owner(state);
    let ticket = state
        .open_ticket(
            OpenTicket {
                root: WORKSPACE.into(),
                draft: TicketDraft {
                    conversation_id: Some(conversation),
                    key: key.into(),
                    title: format!("Task {key}"),
                    request: "Fix the login crash".into(),
                },
            },
            Utc::now(),
        )
        .unwrap()
        .ticket_id;
    state
        .assign_ticket(
            &AssignTicket {
                root: WORKSPACE.into(),
                ticket_id: ticket,
                agent_id: "agent-1".into(),
                workspace_id: WORKSPACE.into(),
            },
            Utc::now(),
        )
        .unwrap();
    ticket
}

fn snapshot(number: u64, state: PullState, items: Vec<PullItem>) -> PullSnapshot {
    PullSnapshot {
        number,
        url: format!("https://github.com/o/r/pull/{number}"),
        title: format!("PR {number}"),
        state,
        head: None,
        checks: Vec::new(),
        items,
    }
}

fn comment(id: &str) -> PullItem {
    PullItem {
        id: id.into(),
        kind: PullItemKind::Comment,
        author: "reviewer".into(),
        own: false,
        access: AuthorAccess::Writer,
        body: format!("please rename {id}"),
        url: format!("https://github.com/o/r/pull/12#{id}"),
        created_at: DateTime::parse_from_rfc3339("2026-11-14T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        path: None,
        line: None,
    }
}

fn report(ticket: TicketId, kind: WorkerReportKind) -> WorkerReport {
    WorkerReport {
        root: WORKSPACE.into(),
        ticket_id: ticket,
        agent_id: "agent-1".into(),
        workspace_id: WORKSPACE.into(),
        kind,
        text: "opened the pull request".into(),
        question: None,
        pull: None,
    }
}

/// Bind pull request `number` to `ticket` through the worker's progress report.
fn bind(state: &mut State, ticket: TicketId, number: u64) {
    let mut bound = report(ticket, WorkerReportKind::Progress);
    bound.pull = Some(snapshot(number, PullState::Open, Vec::new()));
    state.report_progress(&bound, Utc::now()).unwrap();
}

fn read(ticket: TicketId, snapshot: PullSnapshot, live: &[&str]) -> PullRead {
    PullRead {
        root: WORKSPACE.into(),
        ticket,
        snapshot,
        authored: BTreeSet::new(),
        lookups: domain::bot::follow::Lookups::default(),
        live: live.iter().map(|agent| (*agent).to_owned()).collect(),
    }
}

fn answer(ticket: TicketId, text: &str) -> crate::protocol::TicketAnswer {
    crate::protocol::decode(json!({"workspaceId":WORKSPACE,"ticketId":ticket,
        "agentId":"agent-1","answer":text}))
    .unwrap()
}

fn notifications(state: &mut State) -> Vec<Value> {
    state
        .execute(
            "bot.notifications.list.request",
            json!({"workspaceId":WORKSPACE}),
        )
        .unwrap()["notifications"]
        .as_array()
        .unwrap()
        .clone()
}

/// The kinds of the Bot's notifications.
fn kinds(state: &mut State) -> Vec<String> {
    notifications(state)
        .iter()
        .filter_map(|entry| entry["notification"]["kind"].as_str().map(str::to_owned))
        .collect()
}

fn get(state: &mut State, ticket: TicketId) -> Value {
    state
        .execute(
            "bot.ticket.get.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket}),
        )
        .unwrap()["ticket"]
        .clone()
}

#[test]
fn nothing_is_followed_without_a_bound_pull_request() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    ticket(&mut state, "a");

    assert!(state.follow_targets(Utc::now()).is_empty());
}

#[test]
fn a_reported_pull_request_is_bound_projected_and_followed() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");

    bind(&mut state, ticket, 12);

    let targets = state.follow_targets(Utc::now());
    assert_eq!(targets.len(), 1);
    assert_eq!(
        (targets[0].ticket, targets[0].pulls[0].number),
        (ticket, 12)
    );
    assert_eq!(targets[0].root, WORKSPACE);
    let view = get(&mut state, ticket);
    let pull = &view["pulls"][0];
    assert_eq!(pull["number"], 12);
    assert_eq!(pull["boundBy"], "agent");
    assert_eq!(pull["agentId"], "agent-1");
    assert_eq!(pull["checks"]["total"], 0);
    assert!(pull.get("seen").is_none());
    let kinds: Vec<_> = view["reports"]
        .as_array()
        .unwrap()
        .iter()
        .map(|report| report["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("pull"), json!("progress")]);
}

#[test]
fn a_paused_bot_follows_nothing() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    state
        .execute("bot.pause.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();

    assert!(state.follow_targets(Utc::now()).is_empty());
}

#[test]
fn the_workspace_follows_a_bounded_number_of_pull_requests() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let tickets: Vec<TicketId> = (0..5)
        .map(|index| ticket(&mut state, &format!("k{index}")))
        .collect();
    let mut number = 0;
    'bound: for ticket in &tickets {
        for _ in 0..4 {
            if number == FOLLOW_LIMIT as u64 {
                break 'bound;
            }
            number += 1;
            bind(&mut state, *ticket, number);
        }
    }
    let mut over = report(tickets[4], WorkerReportKind::Progress);
    over.pull = Some(snapshot(99, PullState::Open, Vec::new()));

    assert_eq!(
        state.report_progress(&over, Utc::now()),
        Err(ErrorCode::ResourceExhausted)
    );
    over.pull = Some(snapshot(1, PullState::Open, Vec::new()));
    over.ticket_id = tickets[0];
    assert!(
        state.report_progress(&over, Utc::now()).is_ok(),
        "binding again is fine"
    );
}

#[test]
fn review_activity_waits_for_the_live_worker_without_waking_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
    let before = notifications(&mut state).len();

    let deliveries = state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].agent_id, "agent-1");
    assert!(deliveries[0].text.contains("please rename c1"));
    assert_eq!(notifications(&mut state).len(), before);
    assert!(state.follow_targets(Utc::now())[0].pending);
    let again = state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(again, deliveries, "the same read queues nothing new");
}

#[test]
fn activity_without_a_live_worker_and_outcomes_wake_the_ticket_conversation() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &[],
            ),
            Utc::now(),
        )
        .unwrap();
    state
        .observe_pull(
            &read(ticket, snapshot(12, PullState::Closed, Vec::new()), &[]),
            Utc::now(),
        )
        .unwrap();

    let logged = notifications(&mut state);
    let kinds: Vec<_> = logged
        .iter()
        .map(|entry| entry["notification"]["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("pull_closed"), json!("pull_activity")]);
    let conversation = get(&mut state, ticket)["conversationId"].clone();
    assert!(logged.iter().all(
        |entry| entry["notification"]["conversationId"] == conversation
            && entry["notification"]["ticketId"] == json!(ticket)
            && entry["wake"]["state"] == "pending"
    ));
    assert_eq!(
        get(&mut state, ticket)["status"],
        "open",
        "a closed pull request ends nothing"
    );
}

#[test]
fn a_merged_pull_request_ends_the_ticket_and_keeps_following_it() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Merged, Vec::new()),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    let view = get(&mut state, ticket);
    assert_eq!(view["status"], "done");
    assert_eq!(view["outcome"], "Pull request #12 merged");
    assert_eq!(
        notifications(&mut state)[0]["notification"]["kind"],
        "pull_merged"
    );
    let targets = state.follow_targets(Utc::now());
    assert!(targets[0].ended && targets[0].pulls.len() == 1);

    state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Merged, vec![comment("late")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();
    assert_eq!(
        get(&mut state, ticket)["status"],
        "open",
        "a later comment resumes it"
    );
}

#[test]
fn cancelling_a_ticket_by_hand_stops_following_it() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    state
        .execute(
            "bot.ticket.update.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":"cancelled"}),
        )
        .unwrap();

    assert!(state.follow_targets(Utc::now()).is_empty());
}

#[test]
fn pending_inputs_survive_a_restart_until_delivered() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
    let queued = state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();
    drop(state);

    let mut restarted = fixture.state();
    let settled = restarted
        .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
        .unwrap();
    let reread = restarted
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    assert_eq!(settled, queued, "the same input, under the same id");
    assert_eq!(reread, queued, "the handled comment is not queued twice");
    restarted
        .delivered(
            WORKSPACE,
            ticket,
            &[queued[0].id.clone()],
            Utc::now(),
            Utc::now(),
        )
        .unwrap();
    assert!(
        restarted
            .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
            .unwrap()
            .is_empty()
    );
    assert_eq!(restarted.follow_targets(Utc::now())[0].pulls.len(), 1);
    assert!(!restarted.follow_targets(Utc::now())[0].pending);
}

#[test]
fn inputs_for_an_archived_worker_go_to_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
    state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    let left = state
        .settle_pending(WORKSPACE, ticket, &[], Utc::now())
        .unwrap();

    assert!(left.is_empty());
    let logged = notifications(&mut state);
    assert_eq!(logged[0]["notification"]["kind"], "pull_activity");
    assert!(
        logged[0]["notification"]["summary"]
            .as_str()
            .unwrap()
            .contains("please rename c1")
    );
}

#[test]
fn a_failed_read_is_recorded_without_touching_the_ticket() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    state
        .pull_failed(
            WORKSPACE,
            ticket,
            12,
            "gh: not logged in",
            false,
            Utc::now(),
        )
        .unwrap();

    let view = get(&mut state, ticket);
    assert_eq!(view["status"], "open");
    assert_eq!(view["pulls"][0]["error"], "gh: not logged in");
    assert_eq!(state.follow_targets(Utc::now())[0].pulls[0].failures, 1);
}

#[test]
fn the_owner_answers_a_blocked_worker_from_the_client() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    let mut blocked = report(ticket, WorkerReportKind::Blocked);
    blocked.question = Some("Which database?".into());
    state.report_progress(&blocked, Utc::now()).unwrap();
    assert_eq!(
        get(&mut state, ticket)["waiting"]["question"],
        "Which database?"
    );
    let listed = state
        .execute("bot.ticket.list.request", json!({"workspaceId":WORKSPACE}))
        .unwrap();
    assert_eq!(
        listed["tickets"][0]["waiting"]["question"],
        "Which database?"
    );

    let answered = state
        .answer_ticket(&answer(ticket, "Postgres"), Utc::now())
        .unwrap();

    assert_eq!(answered["ticket"]["status"], "open");
    let recorded = answered["ticket"]["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(
        (recorded["source"].clone(), recorded["kind"].clone()),
        (json!("user"), json!("answer"))
    );
    let pending = state
        .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].text.contains("Answer: Postgres"));
    assert_eq!(
        state.answer_ticket(&answer(ticket, "again"), Utc::now()),
        Err(ErrorCode::BotConflict),
        "nobody waits for an answer any more"
    );
}

#[test]
fn the_bot_answers_for_the_user_through_its_update() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    state
        .report_progress(&report(ticket, WorkerReportKind::Blocked), Utc::now())
        .unwrap();

    let updated = state
        .update_ticket(
            WORKSPACE,
            ticket,
            &TicketUpdate {
                answer: Some(domain::bot::ticket::Answer {
                    agent_id: "agent-1".into(),
                    text: "use staging".into(),
                }),
                ..TicketUpdate::default()
            },
            Utc::now(),
        )
        .unwrap();

    let answer = updated["ticket"]["reports"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(answer["source"], "bot");
    assert_eq!(updated["ticket"]["status"], json!(TicketStatus::Open));
}
mod regressions;
mod rules;
mod third_review;
