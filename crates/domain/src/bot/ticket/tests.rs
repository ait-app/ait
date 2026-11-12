use chrono::{DateTime, TimeDelta, Utc};
use serde_json::json;

use super::{
    ReportKind, ReportSource, Ticket, TicketDraft, TicketError, TicketStatus, TicketUpdate,
    WaitReason, WorkerReportKind,
};
use crate::bot::notification::{Notification, NotificationKind};
use crate::{BotId, ConversationId, TicketId};

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-13T08:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + TimeDelta::seconds(seconds)
}

fn draft() -> TicketDraft {
    TicketDraft {
        conversation_id: None,
        key: "create-1".into(),
        title: "  Fix login  ".into(),
        request: "The login page crashes".into(),
    }
}

/// An open ticket with `agent-1` assigned.
fn ticket() -> Ticket {
    let mut ticket = Ticket::open(TicketId::generate(), BotId::generate(), draft(), at(0)).unwrap();
    ticket.assign("agent-1", "wks_1", at(0)).unwrap();
    ticket
}

fn event(kind: NotificationKind, agent: &str) -> Notification {
    serde_json::from_value(json!({
        "id": format!("agent:{agent}:{kind:?}"),
        "workspaceId": "wks_1",
        "kind": kind,
        "agentId": agent,
        "title": "Fix login",
        "summary": "last message",
        "at": "2026-11-13T08:00:00Z"
    }))
    .unwrap()
}

fn status(target: TicketStatus) -> TicketUpdate {
    TicketUpdate {
        status: Some(target),
        ..TicketUpdate::default()
    }
}

#[test]
fn opening_trims_and_bounds_texts_and_refuses_blank_titles_and_bad_keys() {
    let mut long = draft();
    long.title = "\u{e9}".repeat(200);
    long.request = "r".repeat(5000);

    let opened =
        Ticket::open(TicketId::generate(), BotId::generate(), long.clone(), at(0)).unwrap();

    assert_eq!(opened.title.len(), 256);
    assert_eq!(opened.request.len(), 4096);
    assert_eq!(opened.status, TicketStatus::Open);
    assert_eq!(opened.key.as_deref(), Some("create-1"));
    assert!(opened.opened_with(&long));
    assert_eq!(ticket().title, "Fix login");
    for (title, key) in [("  ", "k"), ("t", ""), ("t", "line\nbreak")] {
        let mut invalid = draft();
        invalid.title = title.into();
        invalid.key = key.into();
        assert_eq!(
            Ticket::open(TicketId::generate(), BotId::generate(), invalid, at(0)),
            Err(TicketError::Invalid)
        );
    }
    let mut other = draft();
    other.conversation_id = Some(ConversationId::generate());
    assert!(!ticket().opened_with(&other));
}

#[test]
fn assignments_are_idempotent_and_bounded() {
    let mut ticket = ticket();

    assert!(!ticket.assign("agent-1", "wks_1", at(1)).unwrap());
    for index in 2..=16 {
        assert!(
            ticket
                .assign(&format!("agent-{index}"), "wks_1", at(1))
                .unwrap()
        );
    }

    assert_eq!(
        ticket.assign("agent-17", "wks_1", at(1)),
        Err(TicketError::Full)
    );
    assert_eq!(ticket.assignments.len(), 16);
}

#[test]
fn a_permission_request_makes_an_open_ticket_wait_until_that_agent_settles() {
    let mut ticket = ticket();
    ticket.assign("agent-2", "wks_1", at(0)).unwrap();

    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(1));
    let waiting = ticket.waiting.clone().unwrap();
    ticket.record_notification(&event(NotificationKind::AgentFinished, "agent-2"), at(2));
    let still = ticket.status;
    ticket.record_notification(&event(NotificationKind::AgentFailed, "agent-1"), at(3));

    assert_eq!(waiting.reason, WaitReason::Permission);
    assert_eq!(waiting.agent_id, "agent-1");
    assert_eq!(still, TicketStatus::Waiting);
    assert_eq!(ticket.status, TicketStatus::Open);
    assert_eq!(ticket.waiting, None);
    let kinds: Vec<_> = ticket
        .reports
        .iter()
        .map(|report| (report.kind, report.status))
        .collect();
    assert_eq!(
        kinds,
        [
            (ReportKind::AgentPermission, Some(TicketStatus::Waiting)),
            (ReportKind::AgentFinished, None),
            (ReportKind::AgentFailed, Some(TicketStatus::Open)),
        ]
    );
    assert!(
        ticket
            .reports
            .iter()
            .all(|report| report.source == ReportSource::System)
    );
    assert_eq!(ticket.reports[0].text, "last message");
    assert_eq!(ticket.updated_at, at(3));
}

#[test]
fn notifications_not_about_an_agent_turn_change_nothing() {
    let mut ticket = ticket();
    let before = ticket.clone();

    for kind in [
        NotificationKind::ScheduleSucceeded,
        NotificationKind::ScheduleFailed,
        NotificationKind::TerminalAttention,
        NotificationKind::TicketBlocked,
        NotificationKind::TicketDone,
        NotificationKind::TicketFailed,
    ] {
        ticket.record_notification(&event(kind, "agent-1"), at(1));
    }

    assert_eq!(ticket, before);
}

#[test]
fn a_blocked_worker_waits_until_it_reports_again_not_when_its_turn_ends() {
    let mut ticket = ticket();

    let sequence = ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "Which database?",
            at(1),
        )
        .unwrap();
    ticket.record_notification(&event(NotificationKind::AgentFinished, "agent-1"), at(2));
    let after_turn = ticket.status;
    ticket
        .report(
            "agent-1",
            WorkerReportKind::Progress,
            "Using Postgres",
            at(3),
        )
        .unwrap();

    assert_eq!(sequence, 1);
    assert_eq!(after_turn, TicketStatus::Waiting);
    assert_eq!(ticket.status, TicketStatus::Open);
    assert_eq!(ticket.reports[0].source, ReportSource::Agent);
    assert_eq!(ticket.reports[0].kind, ReportKind::Blocked);
    assert_eq!(ticket.reports[2].status, Some(TicketStatus::Open));
}

#[test]
fn done_and_failed_reports_end_the_reporters_wait_but_never_close() {
    for kind in [WorkerReportKind::Done, WorkerReportKind::Failed] {
        let mut ticket = ticket();
        ticket.assign("agent-2", "wks_1", at(0)).unwrap();
        ticket
            .report("agent-1", WorkerReportKind::Blocked, "stuck", at(1))
            .unwrap();

        ticket.report("agent-2", kind, "other part", at(2)).unwrap();
        let other = ticket.status;
        ticket.report("agent-1", kind, "my part", at(3)).unwrap();

        assert_eq!(other, TicketStatus::Waiting);
        assert_eq!(ticket.status, TicketStatus::Open);
    }
}

#[test]
fn a_waiting_ticket_keeps_its_first_cause() {
    let mut ticket = ticket();
    ticket.assign("agent-2", "wks_1", at(0)).unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Blocked, "stuck", at(1))
        .unwrap();

    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-2"), at(2));
    ticket.record_notification(&event(NotificationKind::AgentFinished, "agent-2"), at(3));

    let waiting = ticket.waiting.unwrap();
    assert_eq!(
        (waiting.reason, waiting.agent_id.as_str()),
        (WaitReason::Blocked, "agent-1")
    );
}

#[test]
fn reports_on_closed_tickets_are_refused_and_unassigned_agents_are_heard() {
    let mut ticket = ticket();

    // The daemon-written label, not the assignment, authorizes a worker (ADR-274 section 7).
    assert_eq!(
        ticket.report("agent-9", WorkerReportKind::Done, "x", at(1)),
        Ok(1)
    );
    assert!(!ticket.assigned("agent-9"));
    ticket
        .update(ReportSource::Bot, &status(TicketStatus::Done), at(2))
        .unwrap();
    assert_eq!(
        ticket.report("agent-1", WorkerReportKind::Progress, "x", at(3)),
        Err(TicketError::Closed)
    );
    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(4));

    assert_eq!(ticket.status, TicketStatus::Done);
    assert_eq!(
        ticket.reports.last().unwrap().kind,
        ReportKind::AgentPermission
    );
}

/// `ticket`'s wait as `(reason, agent)`.
fn waits(ticket: &Ticket) -> Option<(WaitReason, &str)> {
    ticket
        .waiting
        .as_ref()
        .map(|waiting| (waiting.reason, waiting.agent_id.as_str()))
}

#[test]
fn a_blocked_report_upgrades_the_reporters_permission_wait_and_outlives_its_turn() {
    let mut ticket = ticket();
    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(1));

    ticket
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "Need the API key",
            at(2),
        )
        .unwrap();
    let blocked = ticket.clone();
    ticket.record_notification(&event(NotificationKind::AgentFinished, "agent-1"), at(3));

    assert_eq!(waits(&blocked), Some((WaitReason::Blocked, "agent-1")));
    assert_eq!(blocked.waiting.as_ref().unwrap().since, at(2));
    assert_eq!(
        blocked.reports.last().unwrap().status,
        Some(TicketStatus::Waiting)
    );
    assert_eq!(ticket.status, TicketStatus::Waiting);
    assert_eq!(waits(&ticket), Some((WaitReason::Blocked, "agent-1")));
}

#[test]
fn another_agents_block_takes_over_a_permission_wait() {
    let mut ticket = ticket();
    ticket.assign("agent-2", "wks_1", at(0)).unwrap();
    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(1));

    ticket
        .report(
            "agent-2",
            WorkerReportKind::Blocked,
            "Which account?",
            at(2),
        )
        .unwrap();
    ticket.record_notification(&event(NotificationKind::AgentFinished, "agent-1"), at(3));

    assert_eq!(ticket.status, TicketStatus::Waiting);
    assert_eq!(waits(&ticket), Some((WaitReason::Blocked, "agent-2")));
}

#[test]
fn every_blocked_worker_keeps_the_ticket_waiting_until_it_reports_again() {
    let mut ticket = ticket();
    ticket.assign("agent-2", "wks_1", at(0)).unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Blocked, "stuck", at(1))
        .unwrap();
    ticket
        .report("agent-2", WorkerReportKind::Blocked, "stuck too", at(2))
        .unwrap();

    ticket
        .report("agent-1", WorkerReportKind::Progress, "unblocked", at(3))
        .unwrap();
    let handed_over = ticket.clone();
    ticket
        .report("agent-2", WorkerReportKind::Done, "finished", at(4))
        .unwrap();

    assert_eq!(handed_over.status, TicketStatus::Waiting);
    assert_eq!(waits(&handed_over), Some((WaitReason::Blocked, "agent-2")));
    assert_eq!(handed_over.waiting.as_ref().unwrap().since, at(2));
    assert_eq!(
        handed_over.reports.last().unwrap().status,
        Some(TicketStatus::Waiting)
    );
    assert_eq!((ticket.status, ticket.waiting), (TicketStatus::Open, None));

    // The Bot or the user ending the wait clears every block.
    let mut cleared = handed_over;
    cleared
        .update(ReportSource::Bot, &status(TicketStatus::Open), at(5))
        .unwrap();
    cleared.record_notification(&event(NotificationKind::AgentFinished, "agent-1"), at(6));
    cleared
        .report("agent-1", WorkerReportKind::Done, "done", at(7))
        .unwrap();
    assert_eq!(
        (cleared.status, cleared.waiting),
        (TicketStatus::Open, None)
    );
}

#[test]
fn closing_a_closed_ticket_again_with_a_new_outcome_stores_it() {
    let mut ticket = ticket();
    ticket
        .update(ReportSource::User, &status(TicketStatus::Done), at(1))
        .unwrap();
    let mut summary = status(TicketStatus::Done);
    summary.outcome = Some("Shipped in #123".into());

    let stored = ticket.update(ReportSource::Bot, &summary, at(2)).unwrap();
    let repeated = ticket.update(ReportSource::Bot, &summary, at(3)).unwrap();

    assert!(stored);
    assert!(!repeated);
    assert_eq!(ticket.status, TicketStatus::Done);
    assert_eq!(ticket.closed_at, Some(at(1)));
    assert_eq!(ticket.outcome.as_deref(), Some("Shipped in #123"));
    let report = ticket.reports.last().unwrap();
    assert_eq!(
        (
            report.source,
            report.kind,
            report.status,
            report.text.as_str()
        ),
        (
            ReportSource::Bot,
            ReportKind::Status,
            Some(TicketStatus::Done),
            "Shipped in #123"
        )
    );
    assert_eq!(ticket.reports.len(), 2);
}

#[test]
fn ait_closes_finished_work_as_done_and_the_rest_as_cancelled() {
    let reason = "Closed automatically: every Agent on it was archived";
    let mut finished = ticket();
    finished
        .report("agent-1", WorkerReportKind::Done, "fixed", at(1))
        .unwrap();
    finished.record_notification(&event(NotificationKind::AgentFinished, "agent-1"), at(2));
    let mut abandoned = ticket();
    abandoned
        .report("agent-1", WorkerReportKind::Done, "fixed", at(1))
        .unwrap();
    abandoned
        .report(
            "agent-1",
            WorkerReportKind::Progress,
            "one more thing",
            at(2),
        )
        .unwrap();

    assert_eq!(
        finished.close_finished(reason, at(3)),
        Some(TicketStatus::Done)
    );
    assert_eq!(
        abandoned.close_finished(reason, at(3)),
        Some(TicketStatus::Cancelled)
    );
    assert_eq!(
        ticket().close_finished(reason, at(3)),
        Some(TicketStatus::Cancelled)
    );
    assert_eq!(finished.closed_at, Some(at(3)));
    assert_eq!(finished.outcome.as_deref(), Some(reason));
    let report = finished.reports.last().unwrap();
    assert_eq!(
        (
            report.source,
            report.kind,
            report.status,
            report.text.as_str()
        ),
        (
            ReportSource::System,
            ReportKind::Status,
            Some(TicketStatus::Done),
            reason
        )
    );

    // Closed and Agent-less tickets are left alone.
    let mut closed = finished.clone();
    let mut empty = Ticket::open(TicketId::generate(), BotId::generate(), draft(), at(0)).unwrap();
    for unchanged in [&mut closed, &mut empty] {
        let before = unchanged.clone();
        assert_eq!(unchanged.close_finished(reason, at(4)), None);
        assert_eq!(*unchanged, before);
    }
}

#[test]
fn ait_closes_as_done_only_when_every_worker_finished_since_the_last_reopen() {
    let reason = "Closed automatically";
    let close = status(TicketStatus::Done);
    // A `done` from before the ticket was reopened says nothing about the follow-up.
    let mut reopened = ticket();
    reopened
        .report("agent-1", WorkerReportKind::Done, "fixed", at(1))
        .unwrap();
    reopened.update(ReportSource::Bot, &close, at(2)).unwrap();
    reopened.reopen("more work", at(3));
    let mut redone = reopened.clone();
    redone
        .report("agent-1", WorkerReportKind::Done, "follow-up fixed", at(4))
        .unwrap();
    // One worker finished and the other never did.
    let mut half = ticket();
    half.assign("agent-2", "wks_1", at(0)).unwrap();
    half.report("agent-1", WorkerReportKind::Done, "my part", at(1))
        .unwrap();
    // Ending a wait is not a reopen: both workers finished.
    let mut both = half.clone();
    both.report(
        "agent-2",
        WorkerReportKind::Blocked,
        "which account?",
        at(2),
    )
    .unwrap();
    both.update(ReportSource::Bot, &status(TicketStatus::Open), at(3))
        .unwrap();
    both.report("agent-2", WorkerReportKind::Done, "my part", at(4))
        .unwrap();

    assert_eq!(
        reopened.close_finished(reason, at(5)),
        Some(TicketStatus::Cancelled)
    );
    assert_eq!(
        redone.close_finished(reason, at(5)),
        Some(TicketStatus::Done)
    );
    assert_eq!(
        half.close_finished(reason, at(5)),
        Some(TicketStatus::Cancelled)
    );
    assert_eq!(both.close_finished(reason, at(5)), Some(TicketStatus::Done));
}

#[test]
fn ait_cancels_a_waiting_ticket_nobody_can_answer_any_more() {
    let reason = "Closed automatically: it waited on an archived Agent";
    let mut blocked = ticket();
    blocked
        .report(
            "agent-1",
            WorkerReportKind::Blocked,
            "which account?",
            at(1),
        )
        .unwrap();
    let mut permission = ticket();
    permission.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(1));

    for waiting in [&mut blocked, &mut permission] {
        assert_eq!(
            waiting.close_finished(reason, at(2)),
            Some(TicketStatus::Cancelled)
        );
        assert_eq!(
            (waiting.waiting.clone(), waiting.blocked.is_empty()),
            (None, true)
        );
        assert_eq!(waiting.outcome.as_deref(), Some(reason));
        assert_eq!(waiting.closed_at, Some(at(2)));
    }
}

#[test]
fn ait_reopens_a_closed_ticket_with_a_reason() {
    let mut ticket = ticket();
    let mut close = status(TicketStatus::Done);
    close.outcome = Some("Fixed".into());
    ticket.update(ReportSource::Bot, &close, at(1)).unwrap();

    let reopened = ticket.reopen("The Bot gave agent-1 more work", at(2));
    let again = ticket.reopen("The Bot gave agent-1 more work", at(3));

    assert!(reopened);
    assert!(!again);
    assert_eq!(ticket.status, TicketStatus::Open);
    assert_eq!((ticket.closed_at, ticket.outcome.clone()), (None, None));
    let report = ticket.reports.last().unwrap();
    assert_eq!(
        (
            report.source,
            report.kind,
            report.status,
            report.text.as_str()
        ),
        (
            ReportSource::System,
            ReportKind::Status,
            Some(TicketStatus::Open),
            "The Bot gave agent-1 more work"
        )
    );
    assert_eq!(ticket.reports.len(), 2);
}

/// The end of `agent`'s turn of `kind` at `end`, which the daemon says started at `started`.
fn turn_end(
    kind: NotificationKind,
    agent: &str,
    started: Option<DateTime<Utc>>,
    end: DateTime<Utc>,
) -> Notification {
    let mut notification = event(kind, agent);
    notification.turn_started_at = started;
    notification.at = end;
    notification
}

#[test]
fn a_turn_end_is_spared_only_by_its_own_turns_outcome_report() {
    let mut ticket = ticket();
    ticket.assign("agent-2", "wks_1", at(0)).unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Progress, "halfway", at(2))
        .unwrap();
    ticket
        .report("agent-2", WorkerReportKind::Done, "mine", at(3))
        .unwrap();
    ticket
        .report("agent-1", WorkerReportKind::Blocked, "stuck", at(5))
        .unwrap();
    let finished =
        |started, end| turn_end(NotificationKind::AgentFinished, "agent-1", started, end);

    assert!(ticket.spares_turn_end(&finished(Some(at(4)), at(6))));
    assert!(ticket.spares_turn_end(&finished(Some(at(5)), at(5))));
    assert!(!ticket.spares_turn_end(&finished(Some(at(1)), at(4))));
    assert!(!ticket.spares_turn_end(&finished(Some(at(6)), at(9))));
    assert!(!ticket.spares_turn_end(&finished(None, at(9))));
    assert!(ticket.spares_turn_end(&turn_end(
        NotificationKind::AgentFinished,
        "agent-2",
        Some(at(1)),
        at(4)
    )));
    assert!(!ticket.spares_turn_end(&turn_end(
        NotificationKind::AgentFinished,
        "agent-3",
        Some(at(0)),
        at(9)
    )));
    assert!(!ticket.spares_turn_end(&turn_end(
        NotificationKind::AgentPermission,
        "agent-1",
        Some(at(4)),
        at(6)
    )));
}

#[test]
fn a_later_turn_with_a_stale_start_is_never_spared_by_an_earlier_turns_report() {
    let mut ticket = ticket();
    ticket
        .report("agent-1", WorkerReportKind::Done, "fixed", at(2))
        .unwrap();
    let first = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(1)),
        at(3),
    );
    let spared_first = ticket.spares_turn_end(&first);
    ticket.record_notification(&first, at(3));

    // The provider opened the next turn by itself, so the daemon still dates it from at(1).
    let second_finished = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(1)),
        at(6),
    );
    let second_failed = turn_end(NotificationKind::AgentFailed, "agent-1", Some(at(1)), at(6));
    let spared_second = (
        ticket.spares_turn_end(&second_finished),
        ticket.spares_turn_end(&second_failed),
    );
    ticket
        .report("agent-1", WorkerReportKind::Failed, "gave up", at(7))
        .unwrap();
    let third = turn_end(NotificationKind::AgentFailed, "agent-1", Some(at(1)), at(8));

    assert!(spared_first);
    assert_eq!(spared_second, (false, false));
    assert!(ticket.spares_turn_end(&third));
}

#[test]
fn only_the_latest_outcome_of_a_turn_spares_its_end_and_a_failed_end_needs_a_failed_report() {
    let mut recovered = ticket();
    recovered
        .report("agent-1", WorkerReportKind::Blocked, "need the key", at(2))
        .unwrap();
    recovered
        .report("agent-1", WorkerReportKind::Progress, "found it", at(3))
        .unwrap();
    let mut done = ticket();
    done.report("agent-1", WorkerReportKind::Done, "fixed", at(2))
        .unwrap();
    let mut blocked = ticket();
    blocked
        .report("agent-1", WorkerReportKind::Blocked, "need the key", at(2))
        .unwrap();
    let mut failed = ticket();
    failed
        .report("agent-1", WorkerReportKind::Failed, "gave up", at(2))
        .unwrap();
    let finished = turn_end(
        NotificationKind::AgentFinished,
        "agent-1",
        Some(at(1)),
        at(4),
    );
    let crashed = turn_end(NotificationKind::AgentFailed, "agent-1", Some(at(1)), at(4));

    assert!(!recovered.spares_turn_end(&finished));
    assert_eq!(recovered.status, TicketStatus::Open);
    assert!(done.spares_turn_end(&finished));
    assert!(!done.spares_turn_end(&crashed));
    assert!(blocked.spares_turn_end(&finished));
    assert!(!blocked.spares_turn_end(&crashed));
    assert!(failed.spares_turn_end(&finished));
    assert!(failed.spares_turn_end(&crashed));
}

#[test]
fn the_bot_and_the_user_close_reopen_and_end_waits() {
    for target in [
        TicketStatus::Done,
        TicketStatus::Failed,
        TicketStatus::Cancelled,
    ] {
        let mut ticket = ticket();
        ticket
            .report("agent-1", WorkerReportKind::Blocked, "stuck", at(1))
            .unwrap();
        let mut close = status(target);
        close.outcome = Some("Merged #12".into());

        assert!(ticket.update(ReportSource::User, &close, at(2)).unwrap());
        let closed = ticket.clone();
        assert!(
            ticket
                .update(ReportSource::Bot, &status(TicketStatus::Open), at(3))
                .unwrap()
        );

        assert_eq!(closed.status, target);
        assert_eq!(closed.closed_at, Some(at(2)));
        assert_eq!(closed.outcome.as_deref(), Some("Merged #12"));
        assert_eq!(closed.waiting, None);
        let report = closed.reports.last().unwrap();
        assert_eq!(
            (
                report.source,
                report.kind,
                report.status,
                report.text.as_str()
            ),
            (
                ReportSource::User,
                ReportKind::Status,
                Some(target),
                "Merged #12"
            )
        );
        assert_eq!(ticket.status, TicketStatus::Open);
        assert_eq!((ticket.closed_at, ticket.outcome), (None, None));
    }
    let mut waiting = ticket();
    waiting
        .report("agent-1", WorkerReportKind::Blocked, "stuck", at(1))
        .unwrap();
    assert!(
        waiting
            .update(ReportSource::Bot, &status(TicketStatus::Open), at(2))
            .unwrap()
    );
    assert_eq!(
        (waiting.status, waiting.waiting),
        (TicketStatus::Open, None)
    );
}

#[test]
fn repeating_a_status_changes_nothing_and_conflicting_or_malformed_updates_are_refused() {
    let mut ticket = ticket();
    assert!(
        !ticket
            .update(ReportSource::Bot, &status(TicketStatus::Open), at(1))
            .unwrap()
    );
    ticket
        .update(ReportSource::Bot, &status(TicketStatus::Done), at(2))
        .unwrap();
    let closed = ticket.clone();

    assert!(
        !ticket
            .update(ReportSource::User, &status(TicketStatus::Done), at(3))
            .unwrap()
    );
    let mut retitled = status(TicketStatus::Cancelled);
    retitled.title = Some("Other".into());
    assert_eq!(
        ticket.update(ReportSource::User, &retitled, at(3)),
        Err(TicketError::Conflict)
    );
    let outcome_only = TicketUpdate {
        outcome: Some("x".into()),
        ..TicketUpdate::default()
    };
    let blank = TicketUpdate {
        title: Some(" ".into()),
        ..TicketUpdate::default()
    };
    for invalid in [
        TicketUpdate::default(),
        status(TicketStatus::Waiting),
        outcome_only,
        blank,
    ] {
        assert_eq!(
            ticket.update(ReportSource::Bot, &invalid, at(3)),
            Err(TicketError::Invalid)
        );
    }
    let mut reopen_with_outcome = status(TicketStatus::Open);
    reopen_with_outcome.outcome = Some("x".into());
    assert_eq!(
        ticket.update(ReportSource::Bot, &reopen_with_outcome, at(3)),
        Err(TicketError::Invalid)
    );

    assert_eq!(ticket, closed);
}

#[test]
fn titles_and_notes_change_without_touching_the_status() {
    let mut ticket = ticket();
    let update = TicketUpdate {
        title: Some("Fix the login crash".into()),
        note: Some("n".repeat(3000)),
        ..TicketUpdate::default()
    };

    assert!(ticket.update(ReportSource::Bot, &update, at(1)).unwrap());
    let same_title = TicketUpdate {
        title: Some("Fix the login crash".into()),
        ..TicketUpdate::default()
    };

    assert!(
        !ticket
            .update(ReportSource::Bot, &same_title, at(2))
            .unwrap()
    );
    assert_eq!(ticket.title, "Fix the login crash");
    assert_eq!(ticket.status, TicketStatus::Open);
    let note = ticket.reports.last().unwrap();
    assert_eq!(
        (note.kind, note.text.len(), note.status),
        (ReportKind::Note, 2048, None)
    );
}

#[test]
fn only_the_newest_reports_are_kept_with_monotonic_sequences() {
    let mut ticket = ticket();

    for index in 0..40 {
        ticket
            .report(
                "agent-1",
                WorkerReportKind::Progress,
                &index.to_string(),
                at(index),
            )
            .unwrap();
    }

    assert_eq!(ticket.reports.len(), 32);
    assert_eq!(ticket.reports[0].sequence, 9);
    assert_eq!(ticket.reports[31].sequence, 40);
    assert_eq!(ticket.next_report, 41);
}

#[test]
fn records_have_stable_wire_names_and_accept_unknown_fields() {
    let mut ticket = ticket();
    ticket.record_notification(&event(NotificationKind::AgentPermission, "agent-1"), at(1));
    let mut value = serde_json::to_value(&ticket).unwrap();

    assert_eq!(value["status"], "waiting");
    assert_eq!(value["waiting"]["reason"], "permission");
    assert_eq!(value["waiting"]["agentId"], "agent-1");
    assert_eq!(value["assignments"][0]["workspaceId"], "wks_1");
    assert_eq!(value["reports"][0]["source"], "system");
    assert_eq!(value["reports"][0]["kind"], "agent_permission");
    assert_eq!(value["nextReport"], 2);
    value["later"] = json!("field");
    assert_eq!(serde_json::from_value::<Ticket>(value).unwrap(), ticket);
    for (kind, name) in [
        (WorkerReportKind::Progress, None),
        (WorkerReportKind::Blocked, Some("ticket_blocked")),
        (WorkerReportKind::Done, Some("ticket_done")),
        (WorkerReportKind::Failed, Some("ticket_failed")),
    ] {
        assert_eq!(
            kind.notification_kind()
                .map(|kind| serde_json::to_value(kind).unwrap()),
            name.map(|name| json!(name))
        );
    }
    let update: TicketUpdate =
        serde_json::from_value(json!({"status":"cancelled","outcome":"no longer needed"})).unwrap();
    assert_eq!(update.status, Some(TicketStatus::Cancelled));
    assert!(serde_json::from_value::<TicketUpdate>(json!({"actor":"bot"})).is_err());
}
