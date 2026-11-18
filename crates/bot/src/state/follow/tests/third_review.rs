//! Regressions of the third review of tickets that follow pull requests, on the Bot state
//! (ADR-287): failed deliveries counted within a time window, busy replies, and a pull request
//! the forge keeps reporting missing.

use chrono::{TimeDelta, Utc};
use domain::forge::PullState;

use super::{active, bind, comment, get, kinds, read, snapshot, ticket};
use crate::test_support::{Fixture, WORKSPACE};

/// An active state with ticket "a" following #12 and one pending input for `agent-1`; returns
/// the state's ticket and that input's id.
fn pending(state: &mut super::State) -> (domain::TicketId, String) {
    let ticket = ticket(state, "a");
    bind(state, ticket, 12);
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
    (ticket, deliveries[0].id.clone())
}

#[test]
fn an_input_its_agent_fails_to_take_for_the_whole_window_goes_to_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let (ticket, id) = pending(&mut state);
    let start = Utc::now();
    let ids = [id];

    for step in 0..40 {
        state
            .undelivered(
                WORKSPACE,
                ticket,
                &ids,
                &[],
                start + TimeDelta::seconds(15 * step),
            )
            .unwrap();
    }
    assert!(
        !kinds(&mut state).contains(&"pull_activity".to_owned()),
        "a restart shorter than the window strands nothing"
    );
    state
        .undelivered(
            WORKSPACE,
            ticket,
            &ids,
            &[],
            start + TimeDelta::seconds(600),
        )
        .unwrap();

    assert!(kinds(&mut state).contains(&"pull_activity".to_owned()));
    assert!(
        state
            .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
            .unwrap()
            .is_empty()
    );
    assert!(
        get(&mut state, ticket)["reports"]
            .to_string()
            .contains("for 10 minutes")
    );
}

#[test]
fn a_busy_reply_starts_a_new_run_of_failures() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let (ticket, id) = pending(&mut state);
    let start = Utc::now();
    let ids = [id];
    for step in 0..39 {
        state
            .undelivered(
                WORKSPACE,
                ticket,
                &ids,
                &[],
                start + TimeDelta::seconds(15 * step),
            )
            .unwrap();
    }

    state
        .undelivered(
            WORKSPACE,
            ticket,
            &[],
            &ids,
            start + TimeDelta::seconds(590),
        )
        .unwrap();
    for step in 40..60 {
        state
            .undelivered(
                WORKSPACE,
                ticket,
                &ids,
                &[],
                start + TimeDelta::seconds(15 * step),
            )
            .unwrap();
    }

    assert!(!kinds(&mut state).contains(&"pull_activity".to_owned()));
    assert_eq!(
        state
            .settle_pending(WORKSPACE, ticket, &["agent-1".to_owned()], Utc::now())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_pull_request_missing_eight_reads_in_a_row_frees_its_slot_and_wakes_the_bot() {
    let fixture = Fixture::new();
    let mut state = active(&fixture);
    let ticket = ticket(&mut state, "a");
    bind(&mut state, ticket, 12);

    for _ in 0..7 {
        state
            .pull_failed(
                WORKSPACE,
                ticket,
                12,
                "no such pull request",
                true,
                Utc::now(),
            )
            .unwrap();
    }
    assert_eq!(state.follow_targets(Utc::now()).len(), 1);
    assert!(!kinds(&mut state).contains(&"pull_activity".to_owned()));
    state
        .pull_failed(
            WORKSPACE,
            ticket,
            12,
            "no such pull request",
            true,
            Utc::now(),
        )
        .unwrap();

    assert!(
        state.follow_targets(Utc::now()).is_empty(),
        "the slot is free"
    );
    assert!(kinds(&mut state).contains(&"pull_activity".to_owned()));
    let view = get(&mut state, ticket);
    assert!(view["pulls"].as_array().is_none_or(Vec::is_empty));
    assert!(
        view["reports"]
            .to_string()
            .contains("Stopped following pull request #12")
    );
}
