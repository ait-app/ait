//! SQLite adapter of the per-host harness usage ledger (ADR-246).
//!
//! One writer thread owns the connection. Writes and summaries share one bounded queue, so a
//! summary observes every write queued before it, and a full queue drops writes instead of
//! delaying a turn.
//!
//! `daily_usage` keeps every counter per UTC day indefinitely; `calls` and `requests` keep the
//! details for 90 days. The rollup has no outcome dimension, so summaries grouped by outcome
//! cover only the 90-day retention window.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;

use rusqlite::{Connection, Row, Transaction, TransactionBehavior, params};
use tokio::sync::oneshot;

use crate::ports::agent::AgentError;
use crate::ports::usage_ledger::{
    CallBegin, CallFinish, CallOutcome, CallSource, RequestRecord, UsageGroup, UsageKey,
    UsageLedger, UsageLedgerError, UsageQuery, UsageRow, UsageStatus, UsageSummary,
};
use crate::storage::{open, sql};

/// Storage family of the ledger file (`AUSL`).
const FAMILY: i32 = 0x4155_534C;
/// Ledger file relative to the host data directory.
const FILE: &str = "agents/usage.sqlite3";
/// Queued writes and summaries. A turn emits at most a few request frames per second, so 1024
/// absorbs long disk stalls while bounding memory; beyond it writes are dropped with a warning.
const QUEUE_CAPACITY: usize = 1024;
/// One UTC day in milliseconds.
const DAY_MS: i64 = 86_400_000;
/// Latest summary bound, 9999-12-31T00:00:00Z: the last day SQLite `date()` can name.
const LATEST_MS: i64 = 253_402_214_400_000;
/// How long `calls` and `requests` are kept; `daily_usage` is kept indefinitely.
const RETENTION_MS: i64 = 90 * DAY_MS;

/// Every call source, for decoding stored names.
const SOURCES: [CallSource; 6] = [
    CallSource::User,
    CallSource::Bot,
    CallSource::Schedule,
    CallSource::Heartbeat,
    CallSource::Autonomous,
    CallSource::Summary,
];
/// Every call outcome, for decoding stored names.
const OUTCOMES: [CallOutcome; 6] = [
    CallOutcome::Running,
    CallOutcome::Completed,
    CallOutcome::Failed,
    CallOutcome::Cancelled,
    CallOutcome::Rejected,
    CallOutcome::Interrupted,
];

/// Version 1 schema: the ADR-246 tables, the ledger start time and the summary indexes.
const SCHEMA: &str = "
CREATE TABLE meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1), created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE calls (
    call_id TEXT PRIMARY KEY NOT NULL, agent_id TEXT NOT NULL, provider TEXT NOT NULL,
    model TEXT, source TEXT NOT NULL, outcome TEXT NOT NULL, usage_status TEXT,
    started_at INTEGER NOT NULL, ended_at INTEGER, workspace_id TEXT
) STRICT;
CREATE INDEX calls_started ON calls(started_at);
CREATE TABLE requests (
    agent_id TEXT NOT NULL, segment_id TEXT NOT NULL, call_id TEXT NOT NULL, parent TEXT,
    model TEXT, boundary TEXT NOT NULL, input INTEGER NOT NULL, output INTEGER NOT NULL,
    cache_read INTEGER NOT NULL, cache_write INTEGER NOT NULL, cache_write_1h INTEGER,
    reasoning INTEGER, cost_usd_reported REAL, complete INTEGER NOT NULL CHECK(complete IN (0,1)),
    recorded_at INTEGER NOT NULL, PRIMARY KEY(agent_id, segment_id)
) STRICT;
CREATE INDEX requests_call ON requests(call_id);
CREATE INDEX requests_recorded ON requests(recorded_at);
CREATE TABLE daily_usage (
    day TEXT NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL, source TEXT NOT NULL,
    calls INTEGER NOT NULL DEFAULT 0, failed_calls INTEGER NOT NULL DEFAULT 0,
    requests INTEGER NOT NULL DEFAULT 0, input INTEGER NOT NULL DEFAULT 0,
    output INTEGER NOT NULL DEFAULT 0, cache_read INTEGER NOT NULL DEFAULT 0,
    cache_write INTEGER NOT NULL DEFAULT 0, cost_usd_reported REAL NOT NULL DEFAULT 0,
    cost_reported_requests INTEGER NOT NULL DEFAULT 0,
    lower_bound_calls INTEGER NOT NULL DEFAULT 0, missing_calls INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(day, provider, model, source)
) STRICT;
";

/// Insert a running call; a repeated `call_id` changes nothing.
const INSERT_CALL: &str = "INSERT INTO calls(call_id, agent_id, provider, model, source, outcome,
    started_at, workspace_id) VALUES(?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7)
    ON CONFLICT(call_id) DO NOTHING";
/// Count a new call on the rollup row of its start day, provider, model or `''`, and source.
const ROLLUP_CALL: &str = "INSERT INTO daily_usage(day, provider, model, source, calls)
    VALUES(date(?1 / 1000, 'unixepoch'), ?2, ?3, ?4, 1)
    ON CONFLICT(day, provider, model, source) DO UPDATE SET calls = calls + 1";
/// Whether the call a request names was begun.
const CALL_EXISTS: &str = "SELECT EXISTS(SELECT 1 FROM calls WHERE call_id = ?1)";
/// Insert a request; a repeated `(agent_id, segment_id)` changes nothing.
const INSERT_REQUEST: &str = "INSERT INTO requests(agent_id, segment_id, call_id, parent, model,
    boundary, input, output, cache_read, cache_write, cache_write_1h, reasoning,
    cost_usd_reported, complete, recorded_at)
    VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
    ON CONFLICT(agent_id, segment_id) DO NOTHING";
/// Attributes a request to its own model, else the call's model, else `''`.
const ROLLUP_REQUEST: &str = "INSERT INTO daily_usage(day, provider, model, source, requests,
    input, output, cache_read, cache_write, cost_usd_reported, cost_reported_requests)
    SELECT date(?2 / 1000, 'unixepoch'), provider, coalesce(?3, model, ''), source, 1,
        ?4, ?5, ?6, ?7, coalesce(?8, 0), ?8 IS NOT NULL
    FROM calls WHERE call_id = ?1
    ON CONFLICT(day, provider, model, source) DO UPDATE SET
        requests = requests + 1, input = input + excluded.input,
        output = output + excluded.output, cache_read = cache_read + excluded.cache_read,
        cache_write = cache_write + excluded.cache_write,
        cost_usd_reported = cost_usd_reported + excluded.cost_usd_reported,
        cost_reported_requests = cost_reported_requests + excluded.cost_reported_requests";
/// Requests of a call, and how many of them have a complete request boundary.
const REQUEST_COVERAGE: &str = "SELECT count(*),
    count(*) FILTER (WHERE boundary = 'request' AND complete = 1)
    FROM requests WHERE call_id = ?1";
/// Finish a call that is still running; a finished or unknown call changes nothing.
const FINISH_CALL: &str = "UPDATE calls SET outcome = ?2, ended_at = ?3, usage_status = ?4
    WHERE call_id = ?1 AND outcome = 'running'";
/// Count a finished call's failure and status on the rollup row its start was counted on.
const ROLLUP_FINISH: &str = "INSERT INTO daily_usage(day, provider, model, source, failed_calls,
    lower_bound_calls, missing_calls)
    SELECT date(started_at / 1000, 'unixepoch'), provider, coalesce(model, ''), source,
        ?2, ?3, ?4
    FROM calls WHERE call_id = ?1
    ON CONFLICT(day, provider, model, source) DO UPDATE SET
        failed_calls = failed_calls + excluded.failed_calls,
        lower_bound_calls = lower_bound_calls + excluded.lower_bound_calls,
        missing_calls = missing_calls + excluded.missing_calls";
/// Count the status that [`INTERRUPT_RUNNING`] gives each running call on its start-day row.
const ROLLUP_INTERRUPTED: &str = "INSERT INTO daily_usage(day, provider, model, source,
    lower_bound_calls, missing_calls)
    SELECT date(started_at / 1000, 'unixepoch'), provider, coalesce(model, ''), source,
        count(*) FILTER (WHERE EXISTS(
            SELECT 1 FROM requests WHERE requests.call_id = calls.call_id)),
        count(*) FILTER (WHERE NOT EXISTS(
            SELECT 1 FROM requests WHERE requests.call_id = calls.call_id))
    FROM calls WHERE outcome = 'running'
    GROUP BY 1, 2, 3, 4
    ON CONFLICT(day, provider, model, source) DO UPDATE SET
        lower_bound_calls = lower_bound_calls + excluded.lower_bound_calls,
        missing_calls = missing_calls + excluded.missing_calls";
/// An interrupted call may have lost in-flight requests, so it is never exact.
const INTERRUPT_RUNNING: &str = "UPDATE calls SET outcome = 'interrupted', ended_at = ?1,
    usage_status = CASE WHEN EXISTS(SELECT 1 FROM requests WHERE requests.call_id = calls.call_id)
        THEN 'lower_bound' ELSE 'missing' END
    WHERE outcome = 'running'";
/// Requests leave together with their call, so no request outlives the call it is attributed to.
const PRUNE_REQUESTS: &str =
    "DELETE FROM requests WHERE call_id IN (SELECT call_id FROM calls WHERE started_at < ?1)";
/// Delete calls started before the retention cutoff.
const PRUNE_CALLS: &str = "DELETE FROM calls WHERE started_at < ?1";

/// Summary rows from the detail tables for `[?1, ?2)`. Every summary query returns the columns
/// day, provider, model, source, outcome, then the [`UsageRow`] counters in declaration order.
const DETAIL_SUMMARY: &str = "
SELECT date(started_at / 1000, 'unixepoch'), provider, coalesce(model, ''), source, outcome,
    count(*), sum(outcome IS 'failed'), 0, 0, 0, 0, 0, 0.0, 0,
    sum(usage_status IS 'lower_bound'), sum(usage_status IS 'missing')
FROM calls WHERE started_at >= ?1 AND started_at < ?2 GROUP BY 1, 2, 3, 4, 5
UNION ALL
SELECT date(r.recorded_at / 1000, 'unixepoch'), c.provider, coalesce(r.model, c.model, ''),
    c.source, c.outcome, 0, 0, count(*), sum(r.input), sum(r.output), sum(r.cache_read),
    sum(r.cache_write), total(r.cost_usd_reported), count(r.cost_usd_reported), 0, 0
FROM requests r JOIN calls c ON c.call_id = r.call_id
WHERE r.recorded_at >= ?1 AND r.recorded_at < ?2 GROUP BY 1, 2, 3, 4, 5";
/// Summary rows from the rollup for the whole UTC days in `[?1, ?2)`, without an outcome.
const WHOLE_DAY_SUMMARY: &str = "
SELECT day, provider, model, source, NULL, calls, failed_calls, requests, input, output,
    cache_read, cache_write, cost_usd_reported, cost_reported_requests, lower_bound_calls,
    missing_calls
FROM daily_usage
WHERE day >= date(?1 / 1000, 'unixepoch') AND day < date(?2 / 1000, 'unixepoch')";

/// Work for the writer thread, in queue order.
enum Command {
    /// Record the start of a call.
    Begin(CallBegin),
    /// Record one request; boxed to keep queue slots small.
    Request(Box<RequestRecord>),
    /// Record the terminal state of a call.
    Finish(CallFinish),
    /// Mark running calls interrupted as of this Unix time in milliseconds.
    Recover(i64),
    /// Answer a query after every earlier command, replying on the sender.
    Summarize(
        UsageQuery,
        oneshot::Sender<Result<UsageSummary, UsageLedgerError>>,
    ),
    /// Stop the writer after every earlier command.
    Close,
}

/// SQLite usage ledger at `agents/usage.sqlite3`, written by one dedicated thread.
#[derive(Debug)]
pub struct SqliteUsageLedger {
    /// Bounded queue to the writer thread.
    sender: SyncSender<Command>,
    /// Writer thread until [`Self::close`] joins it.
    writer: Mutex<Option<JoinHandle<()>>>,
}

impl SqliteUsageLedger {
    /// Open the ledger under `data_dir` for a starting host.
    ///
    /// Calls that an earlier run left running can no longer finish, so they are queued to be
    /// marked interrupted before any call of this run is recorded. Call [`Self::close`] off the
    /// async runtime when the host stops, because it waits for queued writes.
    ///
    /// # Errors
    /// Rejects foreign, newer or corrupt files and unsafe paths; returns storage failures.
    pub fn open_at_startup(data_dir: &Path) -> Result<Self, AgentError> {
        let ledger = Self::open(data_dir)?;
        ledger.recover_interrupted(chrono::Utc::now().timestamp_millis());
        Ok(ledger)
    }

    /// Open or create the ledger under `data_dir`, prune expired detail rows and start the writer.
    ///
    /// # Errors
    /// Rejects foreign, newer or corrupt files and unsafe paths; returns storage failures.
    pub(crate) fn open(data_dir: &Path) -> Result<Self, AgentError> {
        Self::open_with_clock(data_dir, || chrono::Utc::now().timestamp_millis())
    }

    /// [`Self::open`] with `clock` returning the current Unix time in milliseconds (UTC).
    ///
    /// # Errors
    /// Rejects foreign, newer or corrupt files and unsafe paths; returns storage failures.
    pub(crate) fn open_with_clock(
        data_dir: &Path,
        clock: impl Fn() -> i64 + Send + 'static,
    ) -> Result<Self, AgentError> {
        let path = data_dir.join(FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| AgentError::Io)?;
        }
        let connection = open(&path, FAMILY, SCHEMA, 1)?;
        let now = clock();
        connection
            .execute_batch("PRAGMA journal_mode=WAL;")
            .and_then(|()| {
                connection.execute(
                    "INSERT INTO meta(singleton, created_at) VALUES(1, ?1)
                     ON CONFLICT(singleton) DO NOTHING",
                    [now],
                )
            })
            .map_err(|error| sql(&error))?;
        let since = connection
            .query_row("SELECT created_at FROM meta", [], |row| row.get(0))
            .map_err(|error| sql(&error))?;
        let mut writer = Writer {
            connection,
            clock: Box::new(clock),
            since,
            next_prune: now,
        };
        writer.prune(now).map_err(|error| sql(&error))?;
        let (sender, receiver) = sync_channel(QUEUE_CAPACITY);
        let thread = std::thread::Builder::new()
            .name("usage-ledger".to_owned())
            .spawn(move || writer.run(&receiver))
            .map_err(|_| AgentError::Io)?;
        Ok(Self {
            sender,
            writer: Mutex::new(Some(thread)),
        })
    }

    /// Apply every queued write, then stop the writer and wait for it; later calls do nothing.
    ///
    /// Writes queued afterwards are dropped and summaries report the ledger unavailable. This
    /// blocks while queued writes are applied, so async callers run it on a blocking thread.
    pub fn close(&self) {
        let Some(thread) = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return;
        };
        // Blocks only while the queue is full; a stopped writer has already released it.
        let _ = self.sender.send(Command::Close);
        if thread.join().is_err() {
            tracing::warn!("usage ledger writer panicked");
        }
    }

    /// Queue a write without waiting; a full queue or stopped writer drops it with a warning.
    fn enqueue(&self, command: Command) {
        match self.sender.try_send(command) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!("usage ledger queue is full; dropping a write");
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::warn!("usage ledger writer stopped; dropping a write");
            }
        }
    }
}

impl Drop for SqliteUsageLedger {
    fn drop(&mut self) {
        self.close();
    }
}

#[async_trait::async_trait]
impl UsageLedger for SqliteUsageLedger {
    fn begin_call(&self, call: CallBegin) {
        self.enqueue(Command::Begin(call));
    }

    fn record_request(&self, request: RequestRecord) {
        self.enqueue(Command::Request(Box::new(request)));
    }

    fn finish_call(&self, finish: CallFinish) {
        self.enqueue(Command::Finish(finish));
    }

    fn recover_interrupted(&self, ended_at: i64) {
        self.enqueue(Command::Recover(ended_at));
    }

    async fn summarize(&self, query: UsageQuery) -> Result<UsageSummary, UsageLedgerError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Command::Summarize(query, reply))
            .map_err(|_| UsageLedgerError::Unavailable)?;
        response.await.map_err(|_| UsageLedgerError::Unavailable)?
    }
}

/// State owned by the writer thread.
struct Writer {
    /// The ledger connection; no other thread touches it.
    connection: Connection,
    /// Current Unix time in milliseconds (UTC).
    clock: Box<dyn Fn() -> i64 + Send>,
    /// Ledger start in Unix milliseconds, reported with every summary.
    since: i64,
    /// Earliest time of the next pruning pass.
    next_prune: i64,
}

impl Writer {
    /// Serve queued commands in order until closed or every sender is gone.
    fn run(mut self, receiver: &Receiver<Command>) {
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Begin(call) => self.write(|transaction| begin_call(transaction, &call)),
                Command::Request(request) => {
                    self.write(|transaction| record_request(transaction, &request));
                }
                Command::Finish(finish) => {
                    self.write(|transaction| finish_call(transaction, &finish));
                }
                Command::Recover(ended_at) => {
                    self.write(|transaction| recover_interrupted(transaction, ended_at));
                }
                Command::Summarize(query, reply) => {
                    // The caller may have stopped waiting; then the summary is not needed.
                    let _ = reply.send(summarize(&self.connection, &query, self.since));
                }
                Command::Close => break,
            }
        }
    }

    /// Prune when due, then apply one write in an immediate transaction, logging failures.
    fn write(&mut self, apply: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<()>) {
        let now = (self.clock)();
        if now >= self.next_prune
            && let Err(error) = self.prune(now)
        {
            tracing::warn!(code = ?error.sqlite_error_code(), "usage ledger pruning failed");
        }
        let result = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .and_then(|transaction| {
                apply(&transaction)?;
                transaction.commit()
            });
        if let Err(error) = result {
            tracing::warn!(code = ?error.sqlite_error_code(), "usage ledger write failed");
        }
    }

    /// Delete calls started more than 90 days before `now`, with their requests.
    fn prune(&mut self, now: i64) -> rusqlite::Result<()> {
        let cutoff = now.saturating_sub(RETENTION_MS);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(PRUNE_REQUESTS, [cutoff])?;
        transaction.execute(PRUNE_CALLS, [cutoff])?;
        transaction.commit()?;
        self.next_prune = now.saturating_add(DAY_MS);
        Ok(())
    }
}

/// Insert a running call and count it on its start day; a repeated `call_id` changes nothing.
fn begin_call(transaction: &Transaction<'_>, call: &CallBegin) -> rusqlite::Result<()> {
    let source = call.source.as_str();
    let inserted = transaction.prepare_cached(INSERT_CALL)?.execute(params![
        call.call_id,
        call.agent_id,
        call.provider,
        call.model,
        source,
        call.started_at,
        call.workspace_id,
    ])?;
    if inserted != 0 {
        let model = call.model.as_deref().unwrap_or("");
        transaction.prepare_cached(ROLLUP_CALL)?.execute(params![
            call.started_at,
            call.provider,
            model,
            source,
        ])?;
    }
    Ok(())
}

/// Insert a request of a known call and roll it up only when the row is new.
fn record_request(transaction: &Transaction<'_>, request: &RequestRecord) -> rusqlite::Result<()> {
    let usage = &request.usage;
    if !usage.is_valid() {
        tracing::warn!("usage ledger dropped an invalid request");
        return Ok(());
    }
    let known: bool = transaction
        .prepare_cached(CALL_EXISTS)?
        .query_row([&request.call_id], |row| row.get(0))?;
    if !known {
        tracing::warn!("usage ledger dropped a request for an unknown call");
        return Ok(());
    }
    let inserted = transaction
        .prepare_cached(INSERT_REQUEST)?
        .execute(params![
            request.agent_id,
            usage.segment_id,
            request.call_id,
            usage.parent,
            usage.model,
            usage.boundary.as_str(),
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            usage.cache_write_tokens,
            usage.cache_write_1h_tokens,
            usage.reasoning_tokens,
            usage.cost_usd_reported,
            usage.complete,
            request.recorded_at,
        ])?;
    if inserted != 0 {
        transaction
            .prepare_cached(ROLLUP_REQUEST)?
            .execute(params![
                request.call_id,
                request.recorded_at,
                usage.model,
                usage.input_tokens,
                usage.output_tokens,
                usage.cache_read_tokens,
                usage.cache_write_tokens,
                usage.cost_usd_reported,
            ])?;
    }
    Ok(())
}

/// Finish a running call with its derived [`UsageStatus`] and roll up its failure and status.
fn finish_call(transaction: &Transaction<'_>, finish: &CallFinish) -> rusqlite::Result<()> {
    if finish.outcome == CallOutcome::Running {
        tracing::warn!("usage ledger ignored a call finished as running");
        return Ok(());
    }
    let status = if !finish.token_usage_available {
        UsageStatus::Missing
    } else if finish.outcome == CallOutcome::Interrupted {
        // The host stopped the call, so in-flight requests may be lost, as in recovery.
        match request_status(transaction, &finish.call_id)? {
            UsageStatus::Exact | UsageStatus::LowerBound => UsageStatus::LowerBound,
            UsageStatus::Missing => UsageStatus::Missing,
        }
    } else {
        request_status(transaction, &finish.call_id)?
    };
    let finished = transaction.prepare_cached(FINISH_CALL)?.execute(params![
        finish.call_id,
        finish.outcome.as_str(),
        finish.ended_at,
        status.as_str(),
    ])?;
    if finished == 0 {
        tracing::warn!("usage ledger ignored finishing an unknown or finished call");
        return Ok(());
    }
    let failed = finish.outcome == CallOutcome::Failed;
    let lower_bound = status == UsageStatus::LowerBound;
    let missing = status == UsageStatus::Missing;
    if failed || lower_bound || missing {
        transaction.prepare_cached(ROLLUP_FINISH)?.execute(params![
            finish.call_id,
            failed,
            lower_bound,
            missing,
        ])?;
    }
    Ok(())
}

/// Status of a call whose provider reports token usage, from the requests recorded so far.
fn request_status(transaction: &Transaction<'_>, call_id: &str) -> rusqlite::Result<UsageStatus> {
    let (requests, exact): (u64, u64) = transaction
        .prepare_cached(REQUEST_COVERAGE)?
        .query_row([call_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(if requests == 0 {
        UsageStatus::Missing
    } else if requests == exact {
        UsageStatus::Exact
    } else {
        UsageStatus::LowerBound
    })
}

/// Mark every running call interrupted as of `ended_at` and log how many there were.
fn recover_interrupted(transaction: &Transaction<'_>, ended_at: i64) -> rusqlite::Result<()> {
    transaction.execute(ROLLUP_INTERRUPTED, [])?;
    let interrupted = transaction.execute(INTERRUPT_RUNNING, [ended_at])?;
    if interrupted != 0 {
        tracing::info!(interrupted, "usage ledger marked running calls interrupted");
    }
    Ok(())
}

/// Answer `query`, logging storage failures without their details.
///
/// Grouping by outcome reads only the detail tables, so it covers the 90-day retention window.
fn summarize(
    connection: &Connection,
    query: &UsageQuery,
    since: i64,
) -> Result<UsageSummary, UsageLedgerError> {
    summary_rows(connection, query)
        .map(|rows| UsageSummary { rows, since })
        .map_err(|error| {
            tracing::warn!(code = ?error.sqlite_error_code(), "usage ledger summary failed");
            UsageLedgerError::Failed
        })
}

/// Group the range: whole UTC days from the rollup, partial days from the detail tables.
///
/// Any `i64` range is accepted: it is clamped to `[1970-01-01, 9999-12-31]`, which holds every
/// ledger timestamp and keeps day alignment from overflowing. An empty or inverted range yields
/// no groups.
fn summary_rows(connection: &Connection, query: &UsageQuery) -> rusqlite::Result<Vec<UsageRow>> {
    let from = query.from.clamp(0, LATEST_MS);
    let to = query.to.clamp(0, LATEST_MS);
    let whole_start = (from + DAY_MS - 1) / DAY_MS * DAY_MS;
    let whole_end = to / DAY_MS * DAY_MS;
    let group_by = &query.group_by;
    let mut groups = HashMap::<UsageKey, UsageRow>::new();
    if whole_start < whole_end && !group_by.contains(&UsageGroup::Outcome) {
        let head = (from, whole_start);
        accumulate(connection, DETAIL_SUMMARY, head, group_by, &mut groups)?;
        let whole = (whole_start, whole_end);
        accumulate(connection, WHOLE_DAY_SUMMARY, whole, group_by, &mut groups)?;
        let tail = (whole_end, to);
        accumulate(connection, DETAIL_SUMMARY, tail, group_by, &mut groups)?;
    } else {
        accumulate(
            connection,
            DETAIL_SUMMARY,
            (from, to),
            group_by,
            &mut groups,
        )?;
    }
    let mut rows: Vec<UsageRow> = groups
        .into_iter()
        .map(|(key, row)| UsageRow { key, ..row })
        .collect();
    if rows.is_empty() && query.group_by.is_empty() {
        rows.push(UsageRow::default());
    }
    rows.sort_unstable_by(|left, right| order(&left.key).cmp(&order(&right.key)));
    Ok(rows)
}

/// Add the rows of one summary statement over `[start, end)` to their groups; an empty range
/// adds nothing.
fn accumulate(
    connection: &Connection,
    statement: &str,
    (start, end): (i64, i64),
    group_by: &[UsageGroup],
    groups: &mut HashMap<UsageKey, UsageRow>,
) -> rusqlite::Result<()> {
    if start >= end {
        return Ok(());
    }
    let mut statement = connection.prepare_cached(statement)?;
    let mut rows = statement.query([start, end])?;
    while let Some(row) = rows.next()? {
        let total = groups.entry(group_key(row, group_by)?).or_default();
        total.calls += row.get::<_, u64>(5)?;
        total.failed_calls += row.get::<_, u64>(6)?;
        total.requests += row.get::<_, u64>(7)?;
        total.input_tokens += row.get::<_, u64>(8)?;
        total.output_tokens += row.get::<_, u64>(9)?;
        total.cache_read_tokens += row.get::<_, u64>(10)?;
        total.cache_write_tokens += row.get::<_, u64>(11)?;
        total.reported_cost_usd += row.get::<_, f64>(12)?;
        total.cost_reported_requests += row.get::<_, u64>(13)?;
        total.lower_bound_calls += row.get::<_, u64>(14)?;
        total.missing_calls += row.get::<_, u64>(15)?;
    }
    Ok(())
}

/// Key of a summary row with only the queried dimensions set.
fn group_key(row: &Row<'_>, group_by: &[UsageGroup]) -> rusqlite::Result<UsageKey> {
    let mut key = UsageKey::default();
    for group in group_by {
        match group {
            UsageGroup::Day => key.day = Some(row.get(0)?),
            UsageGroup::Provider => key.provider = Some(row.get(1)?),
            UsageGroup::Model => key.model = Some(row.get(2)?),
            UsageGroup::Source => {
                key.source = Some(decode(
                    &SOURCES,
                    CallSource::as_str,
                    row.get_ref(3)?.as_str()?,
                )?);
            }
            UsageGroup::Outcome => {
                key.outcome = Some(decode(
                    &OUTCOMES,
                    CallOutcome::as_str,
                    row.get_ref(4)?.as_str()?,
                )?);
            }
        }
    }
    Ok(key)
}

/// Find the variant whose stored name is `stored`.
fn decode<T: Copy>(
    variants: &[T],
    name: fn(T) -> &'static str,
    stored: &str,
) -> rusqlite::Result<T> {
    variants
        .iter()
        .copied()
        .find(|variant| name(*variant) == stored)
        .ok_or(rusqlite::Error::InvalidQuery)
}

/// Stable row order: day, provider, model, source, outcome.
type RowOrder<'a> = (
    Option<&'a str>,
    Option<&'a str>,
    Option<&'a str>,
    Option<&'static str>,
    Option<&'static str>,
);

/// Sort key of a summary row.
fn order(key: &UsageKey) -> RowOrder<'_> {
    (
        key.day.as_deref(),
        key.provider.as_deref(),
        key.model.as_deref(),
        key.source.map(CallSource::as_str),
        key.outcome.map(CallOutcome::as_str),
    )
}

#[cfg(test)]
mod tests;
