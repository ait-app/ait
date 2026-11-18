//! Questions, answers and followed pull requests on a ticket (ADR-287).

use std::collections::BTreeSet;

use super::{at, ticket};
use crate::bot::follow::{BoundBy, Lookups};
use crate::bot::notification::NotificationKind;
use crate::bot::ticket::{
    Answer, FOLLOW_AFTER, PENDING_LIMIT, PullObservation, ReportKind, ReportSource, Ticket,
    TicketError, TicketStatus, TicketUpdate, WaitReason, WorkerReportKind,
};
use crate::forge::{AuthorAccess, PullItem, PullItemKind, PullSnapshot, PullState};
use crate::input_envelope::{self, Origin};

pub(super) fn read(number: u64, state: PullState, items: Vec<PullItem>) -> PullSnapshot {
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

pub(super) fn comment(id: &str, own: bool) -> PullItem {
    PullItem {
        id: id.into(),
        kind: PullItemKind::Comment,
        author: "bob".into(),
        own,
        access: AuthorAccess::Writer,
        body: format!("please change {id}"),
        url: format!("https://github.com/o/r/pull/12#{id}"),
        created_at: at(5),
        path: None,
        line: None,
    }
}

pub(super) fn answer(agent: &str, text: &str) -> Answer {
    Answer {
        agent_id: agent.into(),
        text: text.into(),
    }
}

pub(super) fn observe(
    ticket: &mut Ticket,
    snapshot: &PullSnapshot,
    live: &[String],
    reopen: bool,
    seconds: i64,
) -> crate::bot::ticket::Followed {
    ticket
        .observe_pull(
            snapshot.number,
            PullObservation {
                snapshot,
                authored: &BTreeSet::new(),
                lookups: &Lookups::default(),
                live,
                reopen,
                at: at(seconds),
            },
        )
        .unwrap()
}

#[test]
fn a_blocked_worker_asks_a_question_the_wait_shows() {
    let mut ticket = ticket();

    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "need a decision",
            Some("  Which account should I use?  "),
            at(1),
        )
        .unwrap();

    let waiting = ticket.waiting.clone().unwrap();
    assert_eq!(
        waiting.question.as_deref(),
        Some("Which account should I use?")
    );
    assert_eq!(
        serde_json::to_value(&waiting).unwrap()["question"],
        "Which account should I use?"
    );
    assert_eq!(
        ticket.report("agent-1", WorkerReportKind::Done, "x", Some("q"), at(2)),
        Err(TicketError::Invalid)
    );
}

#[test]
fn reporting_blocked_again_replaces_the_question() {
    let mut ticket = ticket();
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "a",
            Some("first?"),
            at(1),
        )
        .unwrap();
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "b",
            Some("second?"),
            at(2),
        )
        .unwrap();

    assert_eq!(
        ticket.waiting.as_ref().unwrap().question.as_deref(),
        Some("second?")
    );
    assert_eq!(ticket.waiting.as_ref().unwrap().since, at(1));
}

#[test]
fn the_wait_hands_over_with_the_next_workers_question() {
    let mut ticket = ticket();
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "a",
            Some("one?"),
            at(1),
        )
        .unwrap();
    ticket
        .report(
            "agent-2",
            WorkerReportKind::Blocked,
            "b",
            Some("two?"),
            at(2),
        )
        .unwrap();

    ticket
        .answer(ReportSource::User, "agent-1", "use staging", at(3))
        .unwrap();

    let waiting = ticket.waiting.clone().unwrap();
    assert_eq!(
        (waiting.agent_id.as_str(), waiting.question.as_deref()),
        ("agent-2", Some("two?"))
    );
}

#[test]
fn an_answer_unblocks_the_worker_and_queues_its_input() {
    let mut ticket = ticket();
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "stuck",
            Some("Which DB?"),
            at(1),
        )
        .unwrap();

    let agent = ticket
        .answer(ReportSource::User, "agent-1", " Postgres ", at(2))
        .unwrap();

    assert_eq!(agent, "agent-1");
    assert_eq!(ticket.status, TicketStatus::Open);
    let answer = ticket.reports.last().unwrap();
    assert_eq!(
        (
            answer.source,
            answer.kind,
            answer.text.as_str(),
            answer.status
        ),
        (
            ReportSource::User,
            ReportKind::Answer,
            "Postgres",
            Some(TicketStatus::Open)
        )
    );
    let delivery = &ticket.pending[0];
    assert_eq!(delivery.agent_id, "agent-1");
    assert_eq!(
        delivery.id,
        format!("answer:{}:{}", ticket.id, answer.sequence)
    );
    let envelope = input_envelope::parse(&delivery.text).unwrap();
    assert_eq!(
        envelope.origin,
        Origin::TicketAnswer {
            ticket_id: ticket.id.to_string()
        }
    );
    assert!(envelope.text.contains("Your question: Which DB?"));
    assert!(envelope.text.contains("Answer: Postgres"));
}

#[test]
fn only_a_ticket_waiting_on_a_blocked_worker_takes_an_answer() {
    let mut open = ticket();
    assert_eq!(
        open.answer(ReportSource::User, "agent-1", "x", at(1)),
        Err(TicketError::Conflict)
    );

    let mut bot = ticket();
    bot.report("agent-1", WorkerReportKind::Blocked, "stuck", None, at(1))
        .unwrap();
    let update = TicketUpdate {
        answer: Some(answer("agent-1", "go ahead")),
        ..TicketUpdate::default()
    };
    assert_eq!(bot.update(ReportSource::Bot, &update, at(2)), Ok(true));
    assert!(bot.pending[0].text.contains("on the user's behalf"));
    let both = TicketUpdate {
        answer: Some(answer("agent-1", "x")),
        status: Some(TicketStatus::Open),
        ..TicketUpdate::default()
    };
    assert_eq!(
        bot.update(ReportSource::Bot, &both, at(3)),
        Err(TicketError::Invalid)
    );
    assert_eq!(
        bot.update(ReportSource::Bot, &update, at(3)),
        Err(TicketError::Conflict)
    );
}

#[test]
fn binding_records_once_and_is_bounded() {
    let mut ticket = ticket();

    assert_eq!(
        ticket.bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1)
        ),
        Ok(true)
    );
    assert_eq!(
        ticket.bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(2)
        ),
        Ok(false)
    );
    for number in 13..16 {
        ticket
            .bind_pull(
                &read(number, PullState::Open, Vec::new()),
                BoundBy::Bot,
                None,
                at(3),
            )
            .unwrap();
    }

    assert_eq!(
        ticket.bind_pull(
            &read(20, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(4)
        ),
        Err(TicketError::Full)
    );
    let bound = &ticket.reports[0];
    assert_eq!(
        (bound.source, bound.kind),
        (ReportSource::Agent, ReportKind::Pull)
    );
    assert!(bound.text.starts_with("Following pull request #12"));
    assert_eq!(ticket.pulls[0].agent_id.as_deref(), Some("agent-1"));
    ticket
        .update(
            ReportSource::User,
            &super::status(TicketStatus::Done),
            at(5),
        )
        .unwrap();
    assert_eq!(
        ticket.bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(6)
        ),
        Err(TicketError::Closed)
    );
}

#[test]
fn worker_events_queue_inputs_for_the_binding_worker_without_notifying_the_bot() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Agent,
            Some("agent-1"),
            at(1),
        )
        .unwrap();
    let live = ["agent-1".to_owned(), "agent-2".to_owned()];

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );

    assert!(followed.changed && followed.notices.is_empty());
    assert_eq!(ticket.pending.len(), 1);
    let delivery = &ticket.pending[0];
    let sequence = ticket.reports.last().unwrap().sequence;
    assert_eq!(
        delivery.id,
        format!("follow:{}:{sequence}:agent-1", ticket.id)
    );
    assert_eq!(
        input_envelope::parse(&delivery.text).unwrap().origin,
        Origin::TicketPull {
            ticket_id: ticket.id.to_string(),
            pull: 12
        }
    );
    assert!(
        ticket
            .reports
            .last()
            .unwrap()
            .text
            .contains("delivered to agent-1")
    );
    let id = delivery.id.clone();
    assert!(ticket.delivered(&[id], at(10), at(11)));
    assert!(ticket.pending.is_empty());
}

#[test]
fn without_a_live_worker_activity_goes_to_the_bot() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &[],
        true,
        10,
    );

    assert!(ticket.pending.is_empty());
    assert_eq!(followed.notices.len(), 1);
    let notice = &followed.notices[0];
    assert_eq!(notice.kind, NotificationKind::PullActivity);
    assert!(
        notice
            .summary
            .as_deref()
            .unwrap()
            .contains("please change c1")
    );
}

#[test]
fn a_full_pending_queue_sends_activity_to_the_bot() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();
    let live: Vec<String> = (0..PENDING_LIMIT)
        .map(|index| format!("agent-{index}"))
        .collect();
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        10,
    );

    let followed = observe(
        &mut ticket,
        &read(
            12,
            PullState::Open,
            vec![comment("c1", false), comment("c2", false)],
        ),
        &live,
        true,
        20,
    );

    assert_eq!(ticket.pending.len(), PENDING_LIMIT);
    assert_eq!(followed.notices[0].kind, NotificationKind::PullActivity);
}

#[test]
fn a_merged_pull_request_ends_the_ticket_and_a_later_comment_resumes_it() {
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

    let merged = observe(
        &mut ticket,
        &read(12, PullState::Merged, Vec::new()),
        &live,
        true,
        10,
    );

    assert!(merged.closed);
    assert_eq!(merged.notices[0].kind, NotificationKind::PullMerged);
    assert_eq!(ticket.status, TicketStatus::Done);
    assert_eq!(ticket.outcome.as_deref(), Some("Pull request #12 merged"));
    assert_eq!(ticket.follow_until, Some(at(10) + FOLLOW_AFTER));
    assert!(ticket.follows_pulls(at(20)));
    assert!(!ticket.follows_pulls(at(10) + FOLLOW_AFTER));

    let own = observe(
        &mut ticket,
        &read(12, PullState::Merged, vec![comment("c1", true)]),
        &live,
        true,
        30,
    );
    assert!(own.notices.is_empty() && ticket.pending.is_empty());
    assert_eq!(
        ticket.status,
        TicketStatus::Done,
        "the forge account never resumes"
    );

    let resumed = observe(
        &mut ticket,
        &read(
            12,
            PullState::Merged,
            vec![comment("c1", true), comment("c2", false)],
        ),
        &live,
        true,
        40,
    );
    assert!(resumed.changed);
    assert_eq!(ticket.status, TicketStatus::Open);
    assert_eq!(
        ticket.follow_until,
        Some(at(40) + FOLLOW_AFTER),
        "a resumed ticket keeps a follow window"
    );
    assert_eq!(ticket.pending.len(), 1);
    assert!(ticket.pending[0].text.contains("please change c2"));
}

#[test]
fn a_comment_at_the_open_limit_goes_to_the_bot_and_the_ticket_stays_closed() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();
    observe(
        &mut ticket,
        &read(12, PullState::Merged, Vec::new()),
        &[],
        true,
        10,
    );

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Merged, vec![comment("c1", false)]),
        &["agent-1".to_owned()],
        false,
        20,
    );

    assert_eq!(ticket.status, TicketStatus::Done);
    assert_eq!(followed.notices[0].kind, NotificationKind::PullActivity);
}

#[test]
fn a_closed_pull_request_tells_the_bot_without_ending_the_ticket() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();

    let followed = observe(
        &mut ticket,
        &read(12, PullState::Closed, Vec::new()),
        &[],
        true,
        10,
    );

    assert_eq!(followed.notices[0].kind, NotificationKind::PullClosed);
    assert_eq!(ticket.status, TicketStatus::Open);
    assert!(!followed.closed);
}

#[test]
fn a_ticket_ends_only_when_every_pull_request_ended() {
    let mut ticket = ticket();
    for number in [12, 13] {
        ticket
            .bind_pull(
                &read(number, PullState::Open, Vec::new()),
                BoundBy::Bot,
                None,
                at(1),
            )
            .unwrap();
    }

    observe(
        &mut ticket,
        &read(12, PullState::Merged, Vec::new()),
        &[],
        true,
        10,
    );
    assert_eq!(ticket.status, TicketStatus::Open);
    let followed = observe(
        &mut ticket,
        &read(13, PullState::Closed, Vec::new()),
        &[],
        true,
        20,
    );

    assert!(followed.closed);
    assert_eq!(ticket.outcome.as_deref(), Some("Pull request #12 merged"));
}

#[test]
fn an_explicit_close_stops_following_and_inputs_of_archived_agents_go_to_the_bot() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();
    observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &["agent-1".to_owned()],
        true,
        10,
    );

    let notices = ticket.reroute_pending(&[], at(20));
    ticket
        .update(
            ReportSource::User,
            &super::status(TicketStatus::Cancelled),
            at(30),
        )
        .unwrap();

    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, NotificationKind::PullActivity);
    assert!(
        notices[0]
            .summary
            .as_deref()
            .unwrap()
            .contains("please change c1")
    );
    assert!(ticket.pending.is_empty());
    assert!(!ticket.follows_pulls(at(40)));
    assert_eq!(
        ticket.observe_pull(
            12,
            PullObservation {
                snapshot: &read(12, PullState::Open, Vec::new()),
                authored: &BTreeSet::new(),
                lookups: &Lookups::default(),
                live: &[],
                reopen: true,
                at: at(40),
            }
        ),
        None
    );
}

#[test]
fn a_failed_read_is_recorded_on_the_binding_only() {
    let mut ticket = ticket();
    ticket
        .bind_pull(
            &read(12, PullState::Open, Vec::new()),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();

    assert!(
        ticket
            .pull_failed(12, "gh: HTTP 502", false, at(2))
            .is_some()
    );
    assert!(ticket.pull_failed(99, "x", false, at(2)).is_none());
    assert_eq!(ticket.status, TicketStatus::Open);
    assert_eq!(ticket.pulls[0].failures, 1);
    assert_eq!(
        ticket.waiting.map(|waiting| waiting.reason),
        None::<WaitReason>
    );
}

#[test]
fn followed_fields_round_trip_and_stay_out_of_tickets_without_them() {
    let mut ticket = ticket();
    let plain = serde_json::to_value(&ticket).unwrap();
    assert!(plain.get("pulls").is_none() && plain.get("pending").is_none());
    ticket
        .bind_pull(
            &read(12, PullState::Open, vec![comment("c1", false)]),
            BoundBy::Bot,
            None,
            at(1),
        )
        .unwrap();

    let value = serde_json::to_value(&ticket).unwrap();
    let loaded: Ticket = serde_json::from_value(value.clone()).unwrap();

    assert_eq!(loaded, ticket);
    assert_eq!(value["pulls"][0]["boundBy"], "bot");
    assert_eq!(value["pulls"][0]["seen"][0], "c1");
}

#[test]
fn a_failing_check_among_many_queues_one_input() {
    use crate::forge::{CheckStatus, PullCheck};
    let checks = |failing: bool| -> Vec<PullCheck> {
        (0..70)
            .map(|index| PullCheck {
                name: format!("job-{index:03}"),
                status: if failing && index == 69 {
                    CheckStatus::Failure
                } else {
                    CheckStatus::Success
                },
                url: None,
                run: None,
            })
            .collect()
    };
    let mut ticket = ticket();
    let mut base = read(12, PullState::Open, Vec::new());
    base.checks = checks(false);
    ticket.bind_pull(&base, BoundBy::Bot, None, at(1)).unwrap();
    let mut failing = base.clone();
    failing.checks = checks(true);
    let live = ["agent-1".to_owned()];

    for round in 0..3 {
        observe(&mut ticket, &failing, &live, true, 60 * (round + 1));
    }

    assert_eq!(ticket.pending.len(), 1);
}

#[test]
fn closing_a_ticket_whose_pull_request_is_open_keeps_following_it() {
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
        .report(
            "agent-1",
            WorkerReportKind::Done,
            "opened PR #12",
            None,
            at(2),
        )
        .unwrap();
    ticket
        .update(ReportSource::Bot, &super::status(TicketStatus::Done), at(3))
        .unwrap();
    assert!(ticket.follows_pulls(at(4)));
    let live = ["agent-1".to_owned()];

    let review = observe(
        &mut ticket,
        &read(12, PullState::Open, vec![comment("c1", false)]),
        &live,
        true,
        60,
    );

    assert!(review.changed);
    assert_eq!(ticket.status, TicketStatus::Open, "review input reopens it");
    assert_eq!(ticket.pending.len(), 1);
    let merged = observe(
        &mut ticket,
        &read(12, PullState::Merged, Vec::new()),
        &live,
        true,
        120,
    );
    assert!(merged.closed);
    assert_eq!(ticket.status, TicketStatus::Done);
}

#[test]
fn cancelling_a_ticket_stops_following_its_open_pull_request() {
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
        .update(
            ReportSource::User,
            &super::status(TicketStatus::Cancelled),
            at(3),
        )
        .unwrap();

    assert!(!ticket.follows_pulls(at(4)));
}
