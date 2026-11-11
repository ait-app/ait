use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use super::*;
use crate::ports::usage_ledger::{RequestUsage, UsageBoundary};

/// One hour in milliseconds.
const HOUR: i64 = 3_600_000;
/// 2025-11-08T00:00:00Z.
const DAY_ONE: i64 = 20_400 * DAY_MS;
/// 2025-11-09T00:00:00Z.
const DAY_TWO: i64 = DAY_ONE + DAY_MS;
/// 2025-11-10T00:00:00Z.
const DAY_THREE: i64 = DAY_TWO + DAY_MS;

/// Ledger under a temporary data directory with a test-controlled clock.
struct Fixture {
    /// Host data directory that holds `agents/usage.sqlite3`.
    directory: tempfile::TempDir,
    /// Current Unix time in milliseconds seen by the ledger's clock.
    now: Arc<AtomicI64>,
}

impl Fixture {
    /// Empty data directory with the clock at `now`.
    fn new(now: i64) -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            now: Arc::new(AtomicI64::new(now)),
        }
    }

    /// Open the ledger with the fixture clock.
    fn open(&self) -> SqliteUsageLedger {
        let now = self.now.clone();
        SqliteUsageLedger::open_with_clock(self.directory.path(), move || {
            now.load(Ordering::SeqCst)
        })
        .unwrap()
    }

    /// Move the ledger's clock to `now`.
    fn set_now(&self, now: i64) {
        self.now.store(now, Ordering::SeqCst);
    }

    /// Read-only view of the ledger file; call after `close` so every write is applied.
    fn inspect(&self) -> Connection {
        Connection::open(self.directory.path().join(FILE)).unwrap()
    }
}

/// User call of `agent-1` without a workspace.
fn call(call_id: &str, provider: &str, model: Option<&str>, started_at: i64) -> CallBegin {
    CallBegin {
        call_id: call_id.into(),
        agent_id: "agent-1".into(),
        provider: provider.into(),
        model: model.map(Into::into),
        source: CallSource::User,
        workspace_id: None,
        started_at,
    }
}

/// Complete request-boundary usage of `agent-1` with a reported cost of 0.25 USD.
fn request(call_id: &str, segment_id: &str, recorded_at: i64) -> RequestRecord {
    RequestRecord {
        call_id: call_id.into(),
        agent_id: "agent-1".into(),
        recorded_at,
        usage: RequestUsage {
            segment_id: segment_id.into(),
            parent: None,
            model: None,
            boundary: UsageBoundary::Request,
            input_tokens: 10,
            output_tokens: 20,
            cache_read_tokens: 30,
            cache_write_tokens: 40,
            cache_write_1h_tokens: None,
            reasoning_tokens: None,
            cost_usd_reported: Some(0.25),
            complete: true,
        },
    }
}

/// Terminal state of a call whose provider reports token usage.
fn finish(call_id: &str, outcome: CallOutcome, ended_at: i64) -> CallFinish {
    CallFinish {
        call_id: call_id.into(),
        outcome,
        ended_at,
        token_usage_available: true,
    }
}

/// Rows of a summary over `[from, to)`.
async fn summary(
    ledger: &SqliteUsageLedger,
    (from, to): (i64, i64),
    group_by: &[UsageGroup],
) -> Vec<UsageRow> {
    ledger
        .summarize(UsageQuery {
            from,
            to,
            group_by: group_by.to_vec(),
        })
        .await
        .unwrap()
        .rows
}

/// `(call_id, outcome, usage_status)` of every call, ordered by `call_id`.
fn statuses(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    connection
        .prepare("SELECT call_id, outcome, usage_status FROM calls ORDER BY call_id")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// A replayed segment is stored and summed once per Agent.
#[tokio::test]
async fn replayed_requests_are_counted_once() {
    let fixture = Fixture::new(DAY_ONE + 12 * HOUR);
    let ledger = fixture.open();
    ledger.begin_call(call("call-1", "claude", Some("sonnet"), DAY_ONE + HOUR));
    ledger.begin_call(call("call-1", "claude", Some("sonnet"), DAY_ONE + HOUR));
    let first = request("call-1", "claude:turn-1:msg_1", DAY_ONE + 2 * HOUR);
    let mut replay = first.clone();
    replay.usage.input_tokens = 99;
    replay.recorded_at += HOUR;
    let mut other_agent = first.clone();
    other_agent.agent_id = "agent-2".into();
    for record in [first.clone(), first, replay, other_agent] {
        ledger.record_request(record);
    }

    let expected = UsageRow {
        calls: 1,
        requests: 2,
        input_tokens: 20,
        output_tokens: 40,
        cache_read_tokens: 60,
        cache_write_tokens: 80,
        reported_cost_usd: 0.5,
        cost_reported_requests: 2,
        ..UsageRow::default()
    };
    let rollup = summary(&ledger, (DAY_ONE, DAY_TWO), &[]).await;
    let detail = summary(&ledger, (DAY_ONE, DAY_TWO - 1), &[]).await;

    assert_eq!(rollup, std::slice::from_ref(&expected));
    assert_eq!(detail, [expected]);
}

/// Ignored and dropped requests leave the rollup unchanged.
#[test]
fn rollup_changes_only_when_a_request_row_is_inserted() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    ledger.begin_call(call("call-1", "codex", Some("gpt"), DAY_ONE + HOUR));
    let stored = request("call-1", "codex:turn-1:1", DAY_ONE + HOUR);
    ledger.record_request(stored.clone());
    let mut replay = stored;
    replay.usage.output_tokens = 500;
    ledger.record_request(replay);
    ledger.record_request(request("unknown-call", "codex:turn-9:1", DAY_ONE + HOUR));
    let mut invalid = request("call-1", "", DAY_ONE + HOUR);
    invalid.usage.segment_id.clear();
    ledger.record_request(invalid);
    ledger.close();

    let connection = fixture.inspect();
    let rollup: Vec<(String, String, String, u64, u64, u64)> = connection
        .prepare("SELECT day, provider, model, calls, requests, output FROM daily_usage")
        .unwrap()
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let requests: u64 = connection
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();

    assert_eq!(
        rollup,
        [("2025-11-08".into(), "codex".into(), "gpt".into(), 1, 1, 20)]
    );
    assert_eq!(requests, 1);
}

/// Finish derives exact, lower-bound and missing statuses and rolls them up.
#[tokio::test]
async fn usage_status_requires_complete_request_boundaries() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    let at = DAY_ONE + HOUR;
    for call_id in [
        "exact",
        "failed",
        "incomplete",
        "none",
        "turn",
        "unavailable",
    ] {
        ledger.begin_call(call(call_id, "pi", Some("model"), at));
    }
    for call_id in ["exact", "failed", "unavailable"] {
        ledger.record_request(request(call_id, &format!("{call_id}:1"), at));
        ledger.record_request(request(call_id, &format!("{call_id}:2"), at));
    }
    let mut incomplete = request("incomplete", "incomplete:1", at);
    incomplete.usage.complete = false;
    ledger.record_request(incomplete);
    let mut turn = request("turn", "turn:1", at);
    turn.usage.boundary = UsageBoundary::Turn;
    ledger.record_request(turn);
    for call_id in ["exact", "incomplete", "none", "turn"] {
        ledger.finish_call(finish(call_id, CallOutcome::Completed, at + HOUR));
    }
    ledger.finish_call(finish("failed", CallOutcome::Failed, at + HOUR));
    ledger.finish_call(finish("failed", CallOutcome::Completed, at + HOUR));
    ledger.finish_call(CallFinish {
        token_usage_available: false,
        ..finish("unavailable", CallOutcome::Completed, at + HOUR)
    });
    // A late request is stored and summed but does not change the finished status.
    let mut late = request("exact", "exact:late", at + 2 * HOUR);
    late.usage.boundary = UsageBoundary::Turn;
    ledger.record_request(late);

    let expected = UsageRow {
        calls: 6,
        failed_calls: 1,
        requests: 9,
        input_tokens: 90,
        output_tokens: 180,
        cache_read_tokens: 270,
        cache_write_tokens: 360,
        reported_cost_usd: 2.25,
        cost_reported_requests: 9,
        lower_bound_calls: 3,
        missing_calls: 2,
        ..UsageRow::default()
    };
    assert_eq!(
        summary(&ledger, (DAY_ONE, DAY_TWO), &[]).await,
        std::slice::from_ref(&expected)
    );
    assert_eq!(
        summary(&ledger, (DAY_ONE, DAY_TWO - 1), &[]).await,
        [expected]
    );
    ledger.close();

    let lower_bound = Some("lower_bound".to_owned());
    let missing = Some("missing".to_owned());
    let exact = Some("exact".to_owned());
    assert_eq!(
        statuses(&fixture.inspect()),
        [
            ("exact".into(), "completed".into(), exact),
            ("failed".into(), "failed".into(), lower_bound.clone()),
            ("incomplete".into(), "completed".into(), lower_bound.clone()),
            ("none".into(), "completed".into(), missing.clone()),
            ("turn".into(), "completed".into(), lower_bound),
            ("unavailable".into(), "completed".into(), missing),
        ]
    );
}

/// A call that ended early is never exact, even when every recorded request is.
#[tokio::test]
async fn calls_that_end_early_are_at_most_lower_bound() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    let at = DAY_ONE + HOUR;
    let outcomes = [
        ("cancelled", CallOutcome::Cancelled),
        ("failed", CallOutcome::Failed),
        ("interrupted", CallOutcome::Interrupted),
    ];
    for (name, outcome) in outcomes {
        let (recorded, silent) = (format!("{name}:recorded"), format!("{name}:silent"));
        ledger.begin_call(call(&recorded, "codex", None, at));
        ledger.begin_call(call(&silent, "codex", None, at));
        ledger.record_request(request(&recorded, &format!("{recorded}:1"), at));
        ledger.finish_call(finish(&recorded, outcome, at + HOUR));
        ledger.finish_call(finish(&silent, outcome, at + HOUR));
    }
    ledger.close();

    let expected: Vec<_> = outcomes
        .iter()
        .flat_map(|(name, _)| {
            [
                (
                    format!("{name}:recorded"),
                    (*name).to_owned(),
                    Some("lower_bound".into()),
                ),
                (
                    format!("{name}:silent"),
                    (*name).to_owned(),
                    Some("missing".into()),
                ),
            ]
        })
        .collect();
    assert_eq!(statuses(&fixture.inspect()), expected);
}

/// Recovery interrupts running calls and leaves finished calls alone.
#[tokio::test]
async fn recovery_interrupts_only_running_calls() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    let at = DAY_ONE + HOUR;
    for call_id in ["done", "partial", "silent"] {
        ledger.begin_call(call(call_id, "omp", None, at));
    }
    ledger.record_request(request("done", "done:1", at));
    ledger.record_request(request("partial", "partial:1", at));
    ledger.finish_call(finish("done", CallOutcome::Completed, at));
    ledger.recover_interrupted(at + HOUR);
    ledger.finish_call(finish("partial", CallOutcome::Completed, at + 2 * HOUR));

    let by_outcome = summary(&ledger, (DAY_ONE, DAY_TWO), &[UsageGroup::Outcome]).await;
    let rollup = summary(&ledger, (DAY_ONE, DAY_TWO), &[]).await;
    ledger.close();

    let outcomes: Vec<_> = by_outcome
        .iter()
        .map(|row| {
            (
                row.key.outcome,
                row.calls,
                row.lower_bound_calls,
                row.missing_calls,
            )
        })
        .collect();
    assert_eq!(
        outcomes,
        [
            (Some(CallOutcome::Completed), 1, 0, 0),
            (Some(CallOutcome::Interrupted), 2, 1, 1),
        ]
    );
    assert_eq!(
        (
            rollup[0].calls,
            rollup[0].lower_bound_calls,
            rollup[0].missing_calls
        ),
        (3, 1, 1)
    );
    let ended: Vec<Option<i64>> = fixture
        .inspect()
        .prepare("SELECT ended_at FROM calls ORDER BY call_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(ended, [Some(at), Some(at + HOUR), Some(at + HOUR)]);
}

/// A starting host interrupts the calls an earlier run left running, before its own calls.
#[test]
fn opening_at_startup_interrupts_calls_an_earlier_run_left_running() {
    let now = chrono::Utc::now().timestamp_millis();
    let fixture = Fixture::new(now);
    let earlier = fixture.open();
    earlier.begin_call(call("done", "codex", None, now - HOUR));
    earlier.finish_call(finish("done", CallOutcome::Completed, now - HOUR));
    earlier.begin_call(call("left", "codex", None, now - HOUR));
    earlier.close();

    let started = SqliteUsageLedger::open_at_startup(fixture.directory.path()).unwrap();
    started.begin_call(call("new", "codex", None, now));
    started.close();

    assert_eq!(
        statuses(&fixture.inspect()),
        [
            ("done".into(), "completed".into(), Some("missing".into())),
            ("left".into(), "interrupted".into(), Some("missing".into())),
            ("new".into(), "running".into(), None),
        ]
    );
}

/// Pruning runs at most daily and never removes rollup rows.
#[tokio::test]
async fn pruning_runs_daily_and_keeps_the_rollup() {
    let now = DAY_THREE + 100 * DAY_MS;
    let old = DAY_ONE + HOUR;
    let fixture = Fixture::new(now);
    let ledger = fixture.open();
    ledger.begin_call(call("old", "claude", Some("opus"), old));
    ledger.record_request(request("old", "old:1", old));
    let old_day = (DAY_ONE, DAY_TWO);

    fixture.set_now(now + DAY_MS - 1);
    ledger.begin_call(call("recent", "claude", Some("opus"), now));
    let before = summary(&ledger, old_day, &[UsageGroup::Outcome]).await;
    fixture.set_now(now + DAY_MS);
    ledger.begin_call(call("next", "claude", Some("opus"), now + DAY_MS));
    let after = summary(&ledger, old_day, &[UsageGroup::Outcome]).await;
    let rollup = summary(&ledger, old_day, &[]).await;

    assert_eq!(
        (before.len(), before[0].calls, before[0].requests),
        (1, 1, 1)
    );
    assert!(after.is_empty());
    assert_eq!((rollup[0].calls, rollup[0].requests), (1, 1));
    ledger.close();
    let calls: Vec<String> = fixture
        .inspect()
        .prepare("SELECT call_id FROM calls ORDER BY call_id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(calls, ["next", "recent"]);
}

/// Opening prunes expired details while the rollup keeps their counts.
#[tokio::test]
async fn opening_prunes_expired_details_and_keeps_rolled_up_statuses() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    for call_id in ["call-1", "silent", "orphan"] {
        ledger.begin_call(call(call_id, "claude", None, DAY_ONE));
    }
    ledger.record_request(request("call-1", "call-1:1", DAY_ONE));
    ledger.finish_call(finish("call-1", CallOutcome::Completed, DAY_ONE));
    ledger.finish_call(finish("silent", CallOutcome::Completed, DAY_ONE));
    ledger.recover_interrupted(DAY_ONE + HOUR);
    ledger.close();
    fixture.set_now(DAY_ONE + RETENTION_MS + 1);

    let reopened = fixture.open();
    let details = summary(&reopened, (DAY_ONE, DAY_TWO), &[UsageGroup::Outcome]).await;
    let rollup = summary(&reopened, (DAY_ONE, DAY_TWO), &[]).await;

    assert!(details.is_empty());
    assert_eq!(
        (rollup[0].calls, rollup[0].requests, rollup[0].missing_calls),
        (3, 1, 2)
    );
    reopened.close();
    let requests: u64 = fixture
        .inspect()
        .query_row("SELECT count(*) FROM requests", [], |row| row.get(0))
        .unwrap();
    assert_eq!(requests, 0);
}

/// Whole days come from the rollup and partial days from the details, consistently.
#[tokio::test]
async fn summaries_split_whole_days_from_partial_days() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    ledger.begin_call(call("late", "claude", Some("opus"), DAY_ONE + 23 * HOUR));
    // Recorded after midnight, so it counts on the second day.
    ledger.record_request(request("late", "late:1", DAY_TWO + HOUR / 2));
    let mut own_model = request("late", "late:2", DAY_TWO + HOUR / 2);
    own_model.usage.model = Some("haiku".into());
    ledger.record_request(own_model);
    ledger.begin_call(call("morning", "codex", None, DAY_TWO + HOUR));
    ledger.record_request(request("morning", "morning:1", DAY_TWO + HOUR));
    ledger.begin_call(call("evening", "claude", Some("opus"), DAY_TWO + 23 * HOUR));
    ledger.begin_call(call("next", "claude", Some("opus"), DAY_THREE + HOUR));

    let last_day = summary(
        &ledger,
        (DAY_ONE + 22 * HOUR, DAY_TWO + 22 * HOUR),
        &[UsageGroup::Day],
    )
    .await;
    let days: Vec<_> = last_day
        .iter()
        .map(|row| (row.key.day.as_deref(), row.calls, row.requests))
        .collect();
    assert_eq!(
        days,
        [(Some("2025-11-08"), 1, 0), (Some("2025-11-09"), 1, 3)]
    );

    let spanning = summary(
        &ledger,
        (DAY_ONE + 12 * HOUR, DAY_THREE + 2 * HOUR),
        &[UsageGroup::Day, UsageGroup::Provider],
    )
    .await;
    let groups: Vec<_> = spanning
        .iter()
        .map(|row| {
            let key = &row.key;
            (
                key.day.as_deref(),
                key.provider.as_deref(),
                row.calls,
                row.requests,
            )
        })
        .collect();
    assert_eq!(
        groups,
        [
            (Some("2025-11-08"), Some("claude"), 1, 0),
            (Some("2025-11-09"), Some("claude"), 1, 2),
            (Some("2025-11-09"), Some("codex"), 1, 1),
            (Some("2025-11-10"), Some("claude"), 1, 0),
        ]
    );

    let dimensions = [UsageGroup::Provider, UsageGroup::Model, UsageGroup::Source];
    let rollup = summary(&ledger, (DAY_TWO, DAY_THREE), &dimensions).await;
    let detail = summary(&ledger, (DAY_TWO, DAY_THREE - 1), &dimensions).await;
    let models: Vec<_> = rollup
        .iter()
        .map(|row| {
            (
                row.key.provider.as_deref(),
                row.key.model.as_deref(),
                row.calls,
            )
        })
        .collect();
    assert_eq!(
        models,
        [
            (Some("claude"), Some("haiku"), 0),
            (Some("claude"), Some("opus"), 1),
            (Some("codex"), Some(""), 1),
        ]
    );
    assert!(
        rollup
            .iter()
            .all(|row| row.key.source == Some(CallSource::User))
    );
    assert_eq!(rollup, detail);
}

/// Summaries carry the first-open time and become unavailable after close.
#[tokio::test]
async fn summaries_report_the_ledger_start_and_stop_after_close() {
    let fixture = Fixture::new(DAY_ONE);
    fixture.open().close();
    fixture.set_now(DAY_TWO);
    let ledger = fixture.open();

    let summary = ledger
        .summarize(UsageQuery {
            from: DAY_ONE,
            to: DAY_THREE,
            group_by: Vec::new(),
        })
        .await
        .unwrap();
    ledger.close();
    ledger.begin_call(call("after-close", "claude", None, DAY_TWO));
    let closed = ledger
        .summarize(UsageQuery {
            from: DAY_ONE,
            to: DAY_THREE,
            group_by: Vec::new(),
        })
        .await;

    assert_eq!(summary.since, DAY_ONE);
    assert_eq!(summary.rows, [UsageRow::default()]);
    assert_eq!(closed, Err(UsageLedgerError::Unavailable));
}

/// Extreme and inverted ranges are answered and the writer keeps running.
#[tokio::test]
async fn extreme_ranges_are_summarized_without_stopping_the_writer() {
    let fixture = Fixture::new(DAY_ONE);
    let ledger = fixture.open();
    ledger.begin_call(call("before", "claude", None, DAY_ONE + HOUR));

    let lowest = summary(&ledger, (i64::MIN, i64::MIN + 1), &[UsageGroup::Day]).await;
    let highest = summary(&ledger, (i64::MAX - 1, i64::MAX), &[]).await;
    let everything = summary(&ledger, (i64::MIN, i64::MAX), &[UsageGroup::Day]).await;
    let inverted = summary(&ledger, (DAY_TWO, DAY_ONE), &[UsageGroup::Day]).await;
    ledger.begin_call(call("after", "claude", None, DAY_ONE));
    let after = summary(&ledger, (DAY_ONE, DAY_TWO), &[]).await;

    assert!(lowest.is_empty());
    assert_eq!(highest, [UsageRow::default()]);
    let days: Vec<_> = everything
        .iter()
        .map(|row| (row.key.day.as_deref(), row.calls))
        .collect();
    assert_eq!(days, [(Some("2025-11-08"), 1)]);
    assert!(inverted.is_empty());
    assert_eq!(after[0].calls, 2);
}

/// Foreign, newer and corrupt files are refused and left untouched.
#[test]
fn refuses_foreign_or_corrupt_files_without_rewriting_them() {
    for schema in [
        "CREATE TABLE legacy(data TEXT);".to_owned(),
        "PRAGMA application_id=1234; PRAGMA user_version=1;".to_owned(),
        format!("PRAGMA application_id={FAMILY}; PRAGMA user_version=2;"),
    ] {
        let fixture = Fixture::new(DAY_ONE);
        let path = fixture.directory.path().join(FILE);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        Connection::open(&path)
            .unwrap()
            .execute_batch(&schema)
            .unwrap();
        let before = std::fs::read(&path).unwrap();

        let result = SqliteUsageLedger::open(fixture.directory.path());

        assert!(
            matches!(result, Err(AgentError::UnsupportedFormat)),
            "{schema}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let fixture = Fixture::new(DAY_ONE);
    let path = fixture.directory.path().join(FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not sqlite").unwrap();
    assert!(matches!(
        SqliteUsageLedger::open(fixture.directory.path()),
        Err(AgentError::UnsupportedFormat)
    ));
}
