//! Regressions of the second review of tickets that follow pull requests (ADR-287): the
//! owner's entries on a ticket closed while its pull request is open, turns that answers and
//! stale deliveries must not spare, automatic closing of a ticket that still follows, and
//! cancelling a closed ticket to stop following.

use super::follow::{comment, observe, read};
use super::{at, status, ticket, turn_end};
use crate::bot::follow::BoundBy;
use crate::bot::notification::NotificationKind;
use crate::bot::ticket::{ReportSource, Ticket, TicketError, TicketStatus, WorkerReportKind};
use crate::forge::PullState;

/// A ticket whose worker bound open pull request #12 and reported `done`, which the Bot then
/// closed as `done`.
fn closed_while_open() -> Ticket {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Done, "opened PR", None, at(2))
        .unwrap();
    ticket
        .update(ReportSource::Bot, &status(TicketStatus::Done), at(3))
        .unwrap();
    assert!(ticket.follows_pulls(at(4)));
    ticket
}

#[test]
fn the_owners_comment_on_a_ticket_closed_while_its_pull_request_is_open_reopens_it() {
    let live = ["agent-1".to_owned()];
    let mut ticket = closed_while_open();

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", true)]),
        &live,
        true,
        60,
    );

    assert!(followed.notices.is_empty());
    assert_eq!(ticket.status, TicketStatus::Open, "the review reopens it");
    assert_eq!(ticket.pending.len(), 1, "and reaches the worker");
    assert!(ticket.pending[0].text.contains("may be your own"));
}

#[test]
fn the_owners_comment_at_the_open_ticket_limit_goes_to_the_bot() {
    let live = ["agent-1".to_owned()];
    let mut ticket = closed_while_open();

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", true)]),
        &live,
        false,
        60,
    );

    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(ticket.pending.is_empty());
    assert_eq!(followed.notices.len(), 1);
    assert_eq!(followed.notices[0].kind, NotificationKind::PullActivity);
}

#[test]
fn an_answer_starts_no_spared_turn_so_the_outcome_wakes_the_bot() {
    let mut ticket = ticket();
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "need",
            Some("which db?"),
            at(1),
        )
        .unwrap();
    ticket
        .answer(ReportSource::User, "agent-1", "postgres", at(2))
        .unwrap();
    let id = ticket.pending[0].id.clone();

    assert!(ticket.delivered(&[id], at(3), at(4)));

    assert!(ticket.input_turns.is_empty(), "an answer is no pull input");
    let silent = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(3)),
        at(60),
    );
    assert!(
        !ticket.spares_turn_end(&silent),
        "a turn after an answer that ends without a report wakes the Bot"
    );
}

#[test]
fn a_pull_input_turn_with_an_outcome_report_falls_back_to_the_report_rule() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    let id = ticket.pending[0].id.clone();
    assert!(ticket.delivered(&[id], at(11), at(13)));
    ticket
        .report("agent-1", WorkerReportKind::Failed, "cannot", None, at(20))
        .unwrap();

    let failed = turn_end(
        NotificationKind::AgentFailed,
        "agent-1",
        Some(at(12)),
        at(30),
    );

    assert!(
        ticket.spares_turn_end(&failed),
        "its own failed report already woke the Bot"
    );
}

#[test]
fn a_delivery_recorded_after_its_turn_ended_starts_no_spared_turn() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    let fast = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(11)),
        at(12),
    );
    assert!(!ticket.spares_turn_end(&fast));
    ticket.record_notification(&fast, at(12));
    let id = ticket.pending[0].id.clone();

    assert!(ticket.delivered(&[id], at(11), at(13)));

    assert!(ticket.input_turns.is_empty(), "the turn it started is over");
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Progress,
            "bot asked",
            None,
            at(100),
        )
        .unwrap();
    let later = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(13)),
        at(110),
    );
    assert!(!ticket.spares_turn_end(&later));
}

#[test]
fn a_turn_that_started_before_the_input_was_sent_is_not_spared() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    let id = ticket.pending[0].id.clone();
    assert!(ticket.delivered(&[id], at(11), at(13)));

    let bots_turn = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(10)),
        at(30),
    );

    assert!(!ticket.spares_turn_end(&bots_turn));
}

#[test]
fn a_turn_end_after_the_send_clears_the_input_turn() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    let id = ticket.pending[0].id.clone();
    assert!(ticket.delivered(&[id], at(11), at(15)));
    assert_eq!(ticket.input_turns.len(), 1);

    let fast = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(12)),
        at(14),
    );
    ticket.record_notification(&fast, at(16));

    assert!(ticket.input_turns.is_empty());
}

#[test]
fn closing_for_ait_keeps_a_ticket_that_follows_an_open_pull_request_as_done() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Progress, "pushed", None, at(2))
        .unwrap();

    assert_eq!(
        ticket.close_finished("archived", at(3)),
        Some(TicketStatus::Done)
    );
    assert!(ticket.follows_pulls(at(4)));
}

#[test]
fn a_closed_ticket_can_be_cancelled_to_stop_following_and_nothing_else() {
    let mut ticket = closed_while_open();

    assert_eq!(
        ticket.update(ReportSource::Bot, &status(TicketStatus::Failed), at(5)),
        Err(TicketError::Conflict)
    );
    assert_eq!(
        ticket.update(ReportSource::Bot, &status(TicketStatus::Cancelled), at(5)),
        Ok(true)
    );

    assert_eq!(ticket.status, TicketStatus::Cancelled);
    assert!(!ticket.follows_pulls(at(6)));
    assert_eq!(
        ticket.update(ReportSource::User, &status(TicketStatus::Done), at(7)),
        Err(TicketError::Conflict)
    );
}

#[test]
fn a_ticket_in_its_post_merge_window_can_be_cancelled() {
    let mut ticket = closed_while_open();
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Merged, Vec::new()),
        &live,
        true,
        60,
    );
    assert!(ticket.follows_pulls(at(61)));

    assert_eq!(
        ticket.update(ReportSource::User, &status(TicketStatus::Cancelled), at(62)),
        Ok(true)
    );

    assert!(!ticket.follows_pulls(at(63)));
}
