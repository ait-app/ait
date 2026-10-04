//! Durable adapter state in `<data_dir>/bonsai/runtime.sqlite3`: runs, tombstones, the
//! neutral event log with its `(epoch, seq)`, accepted inputs and pending requests.
//!
//! Every event is written here before it is sent, so subscriptions replay exactly what live
//! frames carried. The store is the only writer of this file.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};

use crate::wire::{Dispatch, Execution, Person, ReasonCode, RunState};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS runs (
    run_id TEXT PRIMARY KEY,
    dispatch TEXT NOT NULL,
    agent_id TEXT,
    provider TEXT,
    model TEXT,
    approvals INTEGER,
    bonsai_write INTEGER,
    status TEXT NOT NULL,
    reason_code TEXT,
    reason_detail TEXT,
    final_text TEXT,
    status_at INTEGER NOT NULL,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    epoch TEXT NOT NULL,
    next_seq INTEGER NOT NULL DEFAULT 0,
    ait_epoch TEXT,
    ait_seq INTEGER,
    mode TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tombstones (
    run_id TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS events (
    run_id TEXT NOT NULL,
    epoch TEXT NOT NULL,
    seq INTEGER NOT NULL,
    json TEXT NOT NULL,
    PRIMARY KEY (run_id, epoch, seq)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS inputs (
    run_id TEXT NOT NULL,
    input_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL,
    position INTEGER NOT NULL,
    by_id TEXT,
    login TEXT,
    PRIMARY KEY (run_id, input_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS asks (
    run_id TEXT NOT NULL,
    ask_id TEXT NOT NULL,
    spec TEXT NOT NULL,
    state TEXT NOT NULL,
    resolving_by TEXT,
    resolving_effect TEXT,
    PRIMARY KEY (run_id, ask_id)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
";

/// Storage failures; messages carry no paths or content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// `SQLite` failed.
    #[error("runtime storage failed")]
    Sqlite,
    /// A stored row could not be decoded.
    #[error("runtime storage holds an unreadable row")]
    Corrupt,
}

impl From<rusqlite::Error> for StoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sqlite
    }
}

/// One run as recorded locally.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRecord {
    /// Run identity.
    pub run_id: String,
    /// The dispatch frame that created it.
    pub dispatch: Dispatch,
    /// Agent created for the run, once known.
    pub agent_id: Option<String>,
    /// Resolved execution, from `claimed` on.
    pub execution: Option<Execution>,
    /// Current state.
    pub status: RunState,
    /// Failure reason.
    pub reason_code: Option<ReasonCode>,
    /// Failure detail.
    pub reason_detail: Option<String>,
    /// Start of the last assistant message.
    pub final_text: Option<String>,
    /// Local time the state was reached, in milliseconds.
    pub status_at: i64,
    /// A cancel arrived and must be honored, including after a restart.
    pub cancel_requested: bool,
    /// Current event log generation.
    pub epoch: String,
    /// Next sequence number in the generation.
    pub next_seq: u64,
    /// Last translated AIT timeline position.
    pub ait_cursor: Option<(String, u64)>,
    /// Last observed permission mode.
    pub mode: Option<String>,
}

/// A terminal or intermediate status update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusUpdate {
    /// New state.
    pub status: RunState,
    /// Failure reason.
    pub reason_code: Option<ReasonCode>,
    /// Failure detail.
    pub reason_detail: Option<String>,
    /// Start of the last assistant message.
    pub final_text: Option<String>,
    /// Local time in milliseconds.
    pub at: i64,
}

/// An accepted input as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRecord {
    /// Input ID (`input_id`, or the dispatch input's ID).
    pub input_id: String,
    /// Message ID sent to AIT (a UUID derived from the input ID).
    pub message_id: String,
    /// Text to deliver.
    pub text: String,
    /// `queued`, `sent`, `rejected` or `dispatch`.
    pub state: String,
    /// Sender identity.
    pub by: Option<String>,
    /// Sender login.
    pub login: Option<String>,
}

/// A pending or resolved request as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskRecord {
    /// Request ID.
    pub ask_id: String,
    /// Serialized request specification (options and how to answer them).
    pub spec: String,
    /// `pending`, `resolving` or `resolved`.
    pub state: String,
    /// Member resolving it, while `resolving`.
    pub resolving_by: Option<String>,
    /// Effect being applied, while `resolving`.
    pub resolving_effect: Option<String>,
}

/// Shared handle to the adapter database.
#[derive(Debug, Clone)]
pub struct Store(Arc<Mutex<Connection>>);

impl Store {
    /// Open or create the database at `path`, creating parent directories.
    ///
    /// The file holds task text, member messages and tool output, so on Unix its directory is
    /// private to the owner (0700) and the file is 0600; `SQLite` gives its journal files the
    /// same mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Sqlite`] when the file cannot be opened or initialized.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| StoreError::Sqlite)?;
            restrict(parent, 0o700)?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .mode(0o600)
                .open(path)
                .map_err(|_| StoreError::Sqlite)?;
        }
        restrict(path, 0o600)?;
        Self::initialize(Connection::open(path)?)
    }

    /// Open an in-memory database for tests.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Sqlite`] when initialization fails.
    pub fn memory() -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(connection: Connection) -> Result<Self, StoreError> {
        connection.execute_batch(SCHEMA)?;
        // Databases created before inputs recorded their sender.
        let has_sender = connection
            .prepare("SELECT 1 FROM pragma_table_info('inputs') WHERE name = 'by_id'")?
            .exists([])?;
        if !has_sender {
            connection.execute_batch("ALTER TABLE inputs ADD COLUMN by_id TEXT; ALTER TABLE inputs ADD COLUMN login TEXT;")?;
        }
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    /// Remember the machine owner from the last `runtime.welcome`, so input the owner types
    /// before the next connection (after a restart) is attributed to them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn set_owner(&self, owner: &Person) -> Result<(), StoreError> {
        let value = serde_json::to_string(owner).map_err(|_| StoreError::Corrupt)?;
        self.lock().execute(
            "INSERT INTO meta (key, value) VALUES ('owner', ?1)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![value],
        )?;
        Ok(())
    }

    /// The machine owner last learned from Bonsai.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails or the row is unreadable.
    pub fn owner(&self) -> Result<Option<Person>, StoreError> {
        let value: Option<String> = self
            .lock()
            .query_row("SELECT value FROM meta WHERE key = 'owner'", [], |row| {
                row.get(0)
            })
            .optional()?;
        value
            .map(|value| serde_json::from_str(&value).map_err(|_| StoreError::Corrupt))
            .transpose()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Insert a new run in `claimed` state before anything else happens.
    ///
    /// # Returns
    ///
    /// `false` when the run already exists (a duplicate dispatch).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn insert_run(
        &self,
        dispatch: &Dispatch,
        epoch: &str,
        at: i64,
    ) -> Result<bool, StoreError> {
        let text = serde_json::to_string(dispatch).map_err(|_| StoreError::Corrupt)?;
        let changed = self.lock().execute(
            "INSERT OR IGNORE INTO runs (run_id, dispatch, status, status_at, epoch, created_at)
             VALUES (?1, ?2, 'claimed', ?3, ?4, ?3)",
            params![dispatch.run_id, text, at, epoch],
        )?;
        Ok(changed == 1)
    }

    /// Load one run.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails or the row is unreadable.
    pub fn run(&self, run_id: &str) -> Result<Option<RunRecord>, StoreError> {
        self.lock()
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE run_id = ?1"),
                params![run_id],
                read_run,
            )
            .optional()?
            .transpose()
    }

    /// Load every run that has not reached a terminal state.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn open_runs(&self) -> Result<Vec<RunRecord>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare(&format!(
            "SELECT {RUN_COLUMNS} FROM runs WHERE status IN ('claimed', 'running') ORDER BY created_at"
        ))?;
        let rows = statement.query_map([], read_run)?;
        rows.map(|row| row.map_err(StoreError::from).and_then(|run| run))
            .collect()
    }

    /// Every run's current log generation and end, for starting live delivery on a connection.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn ends(&self) -> Result<Vec<(String, String, u64)>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare("SELECT run_id, epoch, next_seq FROM runs")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (run_id, epoch, next) = row?;
            Ok((
                run_id,
                epoch,
                u64::try_from(next).map_err(|_| StoreError::Corrupt)?,
            ))
        })
        .collect()
    }

    /// Record the resolved execution.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn set_execution(&self, run_id: &str, execution: &Execution) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE runs SET provider = ?2, model = ?3, approvals = ?4, bonsai_write = ?5 WHERE run_id = ?1",
            params![
                run_id,
                execution.provider,
                execution.model,
                execution.approvals,
                execution.bonsai_write
            ],
        )?;
        Ok(())
    }

    /// Record the Agent created for a run.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn set_agent(&self, run_id: &str, agent_id: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE runs SET agent_id = ?2 WHERE run_id = ?1",
            params![run_id, agent_id],
        )?;
        Ok(())
    }

    /// Advance a run's state; terminal states never change and states never go back.
    ///
    /// # Returns
    ///
    /// Whether the state advanced.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn advance(&self, run_id: &str, update: &StatusUpdate) -> Result<bool, StoreError> {
        let connection = self.lock();
        let current: Option<String> = connection
            .query_row(
                "SELECT status FROM runs WHERE run_id = ?1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(current) = current.as_deref().and_then(RunState::parse) else {
            return Ok(false);
        };
        if current.is_terminal() || update.status <= current {
            return Ok(false);
        }
        connection.execute(
            "UPDATE runs SET status = ?2, reason_code = ?3, reason_detail = ?4, final_text = ?5, status_at = ?6
             WHERE run_id = ?1",
            params![
                run_id,
                update.status.as_str(),
                update.reason_code.map(ReasonCode::as_str),
                update.reason_detail,
                update.final_text,
                update.at
            ],
        )?;
        Ok(true)
    }

    /// Mark a run for cancellation (honored after a restart too).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn request_cancel(&self, run_id: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE runs SET cancel_requested = 1 WHERE run_id = ?1",
            params![run_id],
        )?;
        Ok(())
    }

    /// Record the last observed permission mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn set_mode(&self, run_id: &str, mode: Option<&str>) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE runs SET mode = ?2 WHERE run_id = ?1",
            params![run_id, mode],
        )?;
        Ok(())
    }

    /// Record a tombstone so a later dispatch of the same run never starts.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn bury(&self, run_id: &str, at: i64) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT OR IGNORE INTO tombstones (run_id, at) VALUES (?1, ?2)",
            params![run_id, at],
        )?;
        Ok(())
    }

    /// Return whether a run was cancelled before this runtime knew it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn is_buried(&self, run_id: &str) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .query_row(
                "SELECT 1 FROM tombstones WHERE run_id = ?1",
                params![run_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Append serialized events to the current generation, in order, starting at `first`.
    ///
    /// Also advances `next_seq` and, when given, the AIT cursor, in one transaction.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails; nothing is written then.
    pub fn append(
        &self,
        run_id: &str,
        epoch: &str,
        first: u64,
        events: &[String],
        ait_cursor: Option<(&str, u64)>,
    ) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO events (run_id, epoch, seq, json) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (offset, json) in events.iter().enumerate() {
                let seq = first + offset as u64;
                insert.execute(params![run_id, epoch, sequence(seq)?, json])?;
            }
        }
        let next = first + events.len() as u64;
        match ait_cursor {
            Some((ait_epoch, ait_seq)) => transaction.execute(
                "UPDATE runs SET next_seq = ?2, ait_epoch = ?3, ait_seq = ?4 WHERE run_id = ?1",
                params![run_id, sequence(next)?, ait_epoch, sequence(ait_seq)?],
            )?,
            None => transaction.execute(
                "UPDATE runs SET next_seq = ?2 WHERE run_id = ?1",
                params![run_id, sequence(next)?],
            )?,
        };
        transaction.commit()?;
        Ok(())
    }

    /// Start a new generation: drop the old events and reset numbering.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn rotate(&self, run_id: &str, epoch: &str) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM events WHERE run_id = ?1", params![run_id])?;
        transaction.execute(
            "UPDATE runs SET epoch = ?2, next_seq = 0, ait_epoch = NULL, ait_seq = NULL WHERE run_id = ?1",
            params![run_id, epoch],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Replace a run's log with `events` (numbered from zero) in a new generation, keeping the
    /// AIT cursor: used when some logged events must never be sent again.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails; nothing changes then.
    pub fn rewrite(&self, run_id: &str, epoch: &str, events: &[String]) -> Result<(), StoreError> {
        let mut connection = self.lock();
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM events WHERE run_id = ?1", params![run_id])?;
        {
            let mut insert = transaction.prepare_cached(
                "INSERT INTO events (run_id, epoch, seq, json) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (seq, json) in events.iter().enumerate() {
                insert.execute(params![run_id, epoch, sequence(seq as u64)?, json])?;
            }
        }
        transaction.execute(
            "UPDATE runs SET epoch = ?2, next_seq = ?3 WHERE run_id = ?1",
            params![run_id, epoch, sequence(events.len() as u64)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Read serialized events `[from, to)` of a generation.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn events(
        &self,
        run_id: &str,
        epoch: &str,
        from: u64,
        to: u64,
    ) -> Result<Vec<String>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare_cached(
            "SELECT json FROM events WHERE run_id = ?1 AND epoch = ?2 AND seq >= ?3 AND seq < ?4 ORDER BY seq",
        )?;
        let rows = statement.query_map(
            params![run_id, epoch, sequence(from)?, sequence(to)?],
            |row| row.get::<_, String>(0),
        )?;
        let events = rows.collect::<Result<Vec<_>, _>>()?;
        if events.len() as u64 != to.saturating_sub(from) {
            return Err(StoreError::Corrupt);
        }
        Ok(events)
    }

    /// Record an accepted input; returns `false` when the ID was seen before.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn insert_input(&self, run_id: &str, input: &InputRecord) -> Result<bool, StoreError> {
        let connection = self.lock();
        let position: i64 = connection.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM inputs WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;
        let changed = connection.execute(
            "INSERT OR IGNORE INTO inputs (run_id, input_id, message_id, text, state, position, by_id, login)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run_id,
                input.input_id,
                input.message_id,
                input.text,
                input.state,
                position,
                input.by,
                input.login
            ],
        )?;
        Ok(changed == 1)
    }

    /// Change an input's state.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn set_input_state(
        &self,
        run_id: &str,
        input_id: &str,
        state: &str,
    ) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE inputs SET state = ?3 WHERE run_id = ?1 AND input_id = ?2",
            params![run_id, input_id, state],
        )?;
        Ok(())
    }

    /// Load a run's inputs in acceptance order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn inputs(&self, run_id: &str) -> Result<Vec<InputRecord>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare_cached(
            "SELECT input_id, message_id, text, state, by_id, login FROM inputs WHERE run_id = ?1 ORDER BY position",
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok(InputRecord {
                input_id: row.get(0)?,
                message_id: row.get(1)?,
                text: row.get(2)?,
                state: row.get(3)?,
                by: row.get(4)?,
                login: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Insert or replace a request.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the write fails.
    pub fn put_ask(&self, run_id: &str, ask: &AskRecord) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT OR REPLACE INTO asks (run_id, ask_id, spec, state, resolving_by, resolving_effect)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_id,
                ask.ask_id,
                ask.spec,
                ask.state,
                ask.resolving_by,
                ask.resolving_effect
            ],
        )?;
        Ok(())
    }

    /// Load a run's requests.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    pub fn asks(&self, run_id: &str) -> Result<Vec<AskRecord>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare_cached(
            "SELECT ask_id, spec, state, resolving_by, resolving_effect FROM asks WHERE run_id = ?1",
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            Ok(AskRecord {
                ask_id: row.get(0)?,
                spec: row.get(1)?,
                state: row.get(2)?,
                resolving_by: row.get(3)?,
                resolving_effect: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

const RUN_COLUMNS: &str = "run_id, dispatch, agent_id, provider, model, approvals, bonsai_write, status, \
     reason_code, reason_detail, final_text, status_at, cancel_requested, epoch, next_seq, ait_epoch, ait_seq, mode";

fn sequence(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Corrupt)
}

type RunRow = Result<RunRecord, StoreError>;

fn read_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    let dispatch: String = row.get(1)?;
    let provider: Option<String> = row.get(3)?;
    let approvals: Option<bool> = row.get(5)?;
    let bonsai_write: Option<bool> = row.get(6)?;
    let status: String = row.get(7)?;
    let reason_code: Option<String> = row.get(8)?;
    let next_seq: i64 = row.get(14)?;
    let ait_epoch: Option<String> = row.get(15)?;
    let ait_seq: Option<i64> = row.get(16)?;
    let execution = match (provider, approvals, bonsai_write) {
        (Some(provider), Some(approvals), Some(bonsai_write)) => Some(Execution {
            provider,
            model: row.get(4)?,
            approvals,
            bonsai_write,
        }),
        _ => None,
    };
    let record = (|| {
        Ok(RunRecord {
            run_id: row.get(0).map_err(StoreError::from)?,
            dispatch: serde_json::from_str(&dispatch).map_err(|_| StoreError::Corrupt)?,
            agent_id: row.get(2).map_err(StoreError::from)?,
            execution,
            status: RunState::parse(&status).ok_or(StoreError::Corrupt)?,
            reason_code: reason_code.as_deref().and_then(ReasonCode::parse),
            reason_detail: row.get(9).map_err(StoreError::from)?,
            final_text: row.get(10).map_err(StoreError::from)?,
            status_at: row.get(11).map_err(StoreError::from)?,
            cancel_requested: row.get(12).map_err(StoreError::from)?,
            epoch: row.get(13).map_err(StoreError::from)?,
            next_seq: u64::try_from(next_seq).map_err(|_| StoreError::Corrupt)?,
            ait_cursor: match (ait_epoch, ait_seq) {
                (Some(epoch), Some(seq)) => {
                    Some((epoch, u64::try_from(seq).map_err(|_| StoreError::Corrupt)?))
                }
                _ => None,
            },
            mode: row.get(17).map_err(StoreError::from)?,
        })
    })();
    Ok(record)
}

/// Set a path's permission bits on Unix; elsewhere the platform's defaults apply.
fn restrict(path: &Path, mode: u32) -> Result<(), StoreError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|_| StoreError::Sqlite)?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[cfg(test)]
mod tests;
