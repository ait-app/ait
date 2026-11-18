//! Regressions of the second review of tickets that follow pull requests, on the Bot state
//! (ADR-287): answered workers' outcomes, closed tickets that still follow, cancelling to free
//! a follow slot and confirmed write access; repeated delivery failures moved to the third
//! review's regressions.

use std::collections::BTreeMap;

use chrono::Utc;
use domain::bot::ticket::WorkerReportKind;
use domain::forge::{AuthorAccess, PullState};
use model::ErrorCode;
use serde_json::json;

use super::{active, answer, bind, comment, get, kinds, read, report, snapshot, ticket};
use crate::follow::FOLLOW_LIMIT;
use crate::state::tickets::CLOSED_LIMIT;
use crate::test_support::{Fixture, WORKSPACE};

fn set_status(state: &mut super::State, ticket: domain::TicketId, status: &str) {
    state
        .execute(
            "bot.ticket.update.request",
            json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":status}),
        )
        .unwrap();
}

#[test]
fn an_answered_workers_done_wakes_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    let mut blocked = report(ticket, WorkerReportKind::Blocked);
    blocked.question = Some("Which database?".into());
    state.report_progress(&blocked, Utc::now()).unwrap();
    state
        .answer_ticket(&answer(ticket, "Postgres"), Utc::now())
        .unwrap();
    let pending = state
        .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
        .unwrap();
    let sent = Utc::now();
    state
        .delivered(
            WORKSPACE,
            ticket,
            &[pending[0].id.clone()],
            sent,
            Utc::now(),
        )
        .unwrap();

    state
        .report_progress(&report(ticket, WorkerReportKind::Done), Utc::now())
        .unwrap();

    assert!(kinds(&mut state).contains(&"ticket_done".to_owned()));
}

#[test]
fn a_closed_ticket_that_still_follows_is_not_evicted() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let following = ticket(&mut state, "following");
    bind(&mut state, following, 12);
    set_status(&mut state, following, "done");

    for index in 0..CLOSED_LIMIT {
        let other = ticket(&mut state, &format!("k{index}"));
        set_status(&mut state, other, "done");
    }

    assert_eq!(get(&mut state, following)["status"], "done");
    assert!(
        state
            .follow_targets(Utc::now())
            .iter()
            .any(|target| target.ticket == following)
    );
}

#[test]
fn closing_for_ait_keeps_a_ticket_that_follows_an_open_pull_request() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    let status = state
        .close_finished_ticket(WORKSPACE, ticket, Utc::now())
        .unwrap();

    assert_eq!(status, Some(domain::bot::ticket::TicketStatus::Done));
    assert!(
        get(&mut state, ticket)["outcome"]
            .as_str()
            .unwrap()
            .contains("keep following")
    );
    assert_eq!(state.follow_targets(Utc::now()).len(), 1);
}

#[test]
fn cancelling_a_closed_ticket_frees_its_follow_slots() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let tickets: Vec<_> = (0..5)
        .map(|index| ticket(&mut state, &format!("k{index}")))
        .collect();
    for number in 1..=FOLLOW_LIMIT as u64 {
        let index = usize::try_from((number - 1) / 4).unwrap();
        bind(&mut state, tickets[index], number);
    }
    set_status(&mut state, tickets[0], "done");
    let mut over = report(tickets[4], WorkerReportKind::Progress);
    over.pull = Some(snapshot(99, PullState::Open, Vec::new()));
    assert_eq!(
        state.report_progress(&over, Utc::now()),
        Err(ErrorCode::ResourceExhausted),
        "a done ticket still follows its open pull requests"
    );

    set_status(&mut state, tickets[0], "cancelled");

    assert!(state.report_progress(&over, Utc::now()).is_ok());
}

#[test]
fn confirmed_write_access_is_kept_and_not_asked_for_again() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
    let mut member = comment("c1");
    member.access = AuthorAccess::Unconfirmed;
    let mut held = read(
        ticket,
        snapshot(12, PullState::Open, vec![member.clone()]),
        &["agent-1"],
    );
    assert!(state.observe_pull(&held, Utc::now()).unwrap().is_empty());

    held.lookups.answered = BTreeMap::from([("reviewer".to_owned(), true)]);
    let deliveries = state.observe_pull(&held, Utc::now()).unwrap();

    assert_eq!(deliveries.len(), 1);
    let mut later = comment("c2");
    later.access = AuthorAccess::Unconfirmed;
    later.created_at = member.created_at + chrono::TimeDelta::minutes(5);
    let next = snapshot(12, PullState::Open, vec![member, later]);
    assert!(
        state
            .authors_to_confirm(WORKSPACE, ticket, &next)
            .unwrap()
            .is_empty(),
        "the kept answer is not looked up again"
    );
}
