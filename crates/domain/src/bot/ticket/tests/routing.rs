//! Who hears of a followed pull request's activity, turns that delivered inputs start, answers
//! that name their wait, and tickets that resume after their pull requests ended (ADR-287).

use super::follow::{answer, comment, observe, read};
use super::{at, ticket, turn_end};
use crate::bot::follow::BoundBy;
use crate::bot::notification::NotificationKind;
use crate::bot::ticket::{ReportSource, TicketError, TicketStatus, TicketUpdate, WorkerReportKind};
use crate::forge::{AuthorAccess, PullItem, PullState};

fn by(id: &str, author: &str, access: AuthorAccess) -> PullItem {
    let mut item = comment(id, false);
    item.author = author.into();
    item.access = access;
    item
}

fn followed_ticket(state: PullState) -> crate::bot::ticket::Ticket {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    if state != PullState::Open {
        observe(&mut ticket, &read(12, state, Vec::new()), &[], true, 2);
    }
    ticket
}

#[test]
fn entries_from_outside_the_repository_reach_only_the_bot_and_bots_reach_no_one() {
    let mut ticket = followed_ticket(PullState::Open);
    let live = ["agent-1".to_owned()];

    let followed = observe(
        &mut ticket,
        &read(
            12,
            PullState::Open,
            vec![
                by("c1", "stranger", AuthorAccess::Other),
                by("c2", "visitor", AuthorAccess::Other),
                by("c3", "codecov", AuthorAccess::Bot),
            ],
        ),
        &live,
        true,
        10,
    );

    assert!(ticket.pending.is_empty(), "nothing reaches the worker");
    assert_eq!(followed.notices.len(), 1, "one coalesced notice");
    let notice = &followed.notices[0];
    assert_eq!(notice.kind, NotificationKind::PullActivity);
    let summary = notice.summary.as_deref().unwrap();
    assert!(summary.contains("stranger") && summary.contains("visitor"));
    assert!(
        !summary.contains("codecov"),
        "bot accounts are not reported"
    );
    assert!(!summary.contains("please change"), "their text stays out");
}

#[test]
fn outside_and_bot_entries_never_reopen_an_ended_ticket() {
    let mut ticket = followed_ticket(PullState::Merged);
    assert_eq!(ticket.status, TicketStatus::Done);
    let live = ["agent-1".to_owned()];

    let followed = observe(
        &mut ticket,
        &read(
            12,
            PullState::Merged,
            vec![
                by("c1", "release-bot", AuthorAccess::Bot),
                by("c2", "stranger", AuthorAccess::Other),
            ],
        ),
        &live,
        true,
        10,
    );

    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(ticket.pending.is_empty());
    assert_eq!(followed.notices.len(), 1);
    assert!(
        followed.notices[0]
            .summary
            .as_deref()
            .unwrap()
            .contains("stranger")
    );
}

#[test]
fn a_ticket_resumed_on_ended_pull_requests_ends_again_on_done_or_when_the_window_passes() {
    let mut ticket = followed_ticket(PullState::Merged);
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Merged, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    assert_eq!(ticket.status, TicketStatus::Open);
    assert!(ticket.pulls_ended());

    ticket
        .report("agent-1", WorkerReportKind::Done, "addressed", None, at(20))
        .unwrap();

    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(ticket.follows_pulls(at(30)), "the window still runs");
    observe(
        &mut ticket,
        &read(
            12,
            PullState::Merged,
            vec![comment("c1", false), comment("c2", false)],
        ),
        &live,
        true,
        40,
    );
    assert_eq!(ticket.status, TicketStatus::Open);
    let until = ticket.follow_until.unwrap();
    assert!(!ticket.expire_follow(until - chrono::TimeDelta::seconds(1)));

    assert!(ticket.expire_follow(until));

    assert_eq!(ticket.status, TicketStatus::Done);
    assert!(!ticket.follows_pulls(until));
}

#[test]
fn a_turn_a_delivered_input_started_ends_without_waking_the_bot() {
    let mut ticket = followed_ticket(PullState::Open);
    let live = ["agent-1".to_owned()];
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );
    let id = ticket.pending[0].id.clone();
    assert!(ticket.delivered(&[id], at(11), at(12)));
    assert_eq!(ticket.input_turns.len(), 1);
    assert_eq!(ticket.input_turns[0].agent_id, "agent-1");

    let failed = turn_end(
        NotificationKind::AgentFailed,
        "agent-1",
        Some(at(11)),
        at(20),
    );
    let finished = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(11)),
        at(20),
    );
    assert!(
        !ticket.spares_turn_end(&failed),
        "a failed turn still wakes the Bot"
    );
    assert!(ticket.spares_turn_end(&finished));
    ticket.record_notification(&finished, at(20));

    let later = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(30)),
        at(40),
    );
    assert!(ticket.input_turns.is_empty(), "the turn end cleared it");
    assert!(
        !ticket.spares_turn_end(&later),
        "the user's next turn wakes it as before"
    );
}

#[test]
fn an_answer_must_name_the_worker_the_ticket_still_waits_on() {
    let mut ticket = ticket();
    for agent in ["agent-1", "agent-2"] {
        ticket
            .report(
                agent,
                WorkerReportKind::Blocked,
                "stuck",
                Some("which?"),
                at(1),
            )
            .unwrap();
    }
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Progress,
            "unblocked myself",
            None,
            at(2),
        )
        .unwrap();
    assert_eq!(ticket.waiting.as_ref().unwrap().agent_id, "agent-2");

    let stale = TicketUpdate {
        answer: Some(answer("agent-1", "use staging")),
        ..TicketUpdate::default()
    };

    assert_eq!(
        ticket.update(ReportSource::Bot, &stale, at(3)),
        Err(TicketError::Conflict)
    );
    assert_eq!(
        ticket.answer(ReportSource::User, "agent-1", "use staging", at(3)),
        Err(TicketError::Conflict)
    );
    assert!(ticket.pending.is_empty());
    assert_eq!(
        ticket.answer(ReportSource::User, "agent-2", "use staging", at(3)),
        Ok("agent-2".to_owned())
    );
}

#[test]
fn a_cancelled_ticket_keeps_its_pull_requests_but_does_not_end_on_their_merge() {
    let mut ticket = followed_ticket(PullState::Open);
    ticket
        .update(
            ReportSource::Bot,
            &super::status(TicketStatus::Cancelled),
            at(5),
        )
        .unwrap();

    assert_eq!(ticket.pulls.len(), 1);
    assert!(!ticket.follows_pulls(at(6)));
    assert_eq!(ticket.follow_until, None);
}
