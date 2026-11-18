//! Following after a close, the follow limit on reopening, outside authors and turns that
//! delivered inputs start, on the Bot state (ADR-287).

use chrono::Utc;
use domain::bot::ticket::WorkerReportKind;
use domain::forge::{AuthorAccess, PullState};
use model::ErrorCode;
use serde_json::json;

use super::{active, bind, comment, kinds, read, report, snapshot, ticket};
use crate::follow::FOLLOW_LIMIT;
use crate::test_support::WORKSPACE;

fn update(
    state: &mut super::State,
    ticket: domain::TicketId,
    status: &str,
) -> Result<serde_json::Value, ErrorCode> {
    state.execute(
        "bot.ticket.update.request",
        json!({"workspaceId":WORKSPACE,"ticketId":ticket,"status":status}),
    )
}

#[test]
fn a_ticket_closed_while_its_pull_request_is_open_keeps_being_followed() {
    let fixture = crate::test_support::Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    update(&mut state, ticket, "done").unwrap();

    let targets = state.follow_targets(Utc::now());
    assert_eq!(
        targets.len(),
        1,
        "done keeps following the open pull request"
    );
    assert!(!targets[0].ended);
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
    assert_eq!(super::get(&mut state, ticket)["status"], "open");
}

#[test]
fn reopening_beyond_the_follow_limit_is_refused() {
    let fixture = crate::test_support::Fixture::new();
    let mut state = active(&fixture);
    let tickets: Vec<_> = (0..5)
        .map(|index| ticket(&mut state, &format!("k{index}")))
        .collect();
    let mut number = 0;
    for ticket in &tickets[..4] {
        for _ in 0..4 {
            number += 1;
            bind(&mut state, *ticket, number);
        }
    }
    update(&mut state, tickets[0], "cancelled").unwrap();
    for _ in 0..4 {
        number += 1;
        bind(&mut state, tickets[4], number);
    }
    assert_eq!(number, FOLLOW_LIMIT as u64 + 4);

    assert_eq!(
        update(&mut state, tickets[0], "open"),
        Err(ErrorCode::ResourceExhausted)
    );
}

#[test]
fn entries_from_outside_the_repository_wake_the_bot_and_never_reach_the_worker() {
    let fixture = crate::test_support::Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
    let mut stranger = comment("c1");
    stranger.access = AuthorAccess::Other;

    let deliveries = state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Open, vec![stranger]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    assert!(deliveries.is_empty());
    assert!(kinds(&mut state).contains(&"pull_activity".to_owned()));
}

#[test]
fn a_done_report_wakes_the_bot_also_in_a_turn_a_delivered_input_started() {
    let fixture = crate::test_support::Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);
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
    let sent = Utc::now();
    state
        .delivered(
            WORKSPACE,
            ticket,
            &[deliveries[0].id.clone()],
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
fn every_pull_request_ended_is_read_at_the_slower_pace() {
    let fixture = crate::test_support::Fixture::new();
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
    state
        .observe_pull(
            &read(
                ticket,
                snapshot(12, PullState::Merged, vec![comment("c1")]),
                &["agent-1"],
            ),
            Utc::now(),
        )
        .unwrap();

    let targets = state.follow_targets(Utc::now());

    assert_eq!(super::get(&mut state, ticket)["status"], "open", "resumed");
    assert!(
        targets[0].ended,
        "a resumed ticket is still read at the ended pace"
    );
}
