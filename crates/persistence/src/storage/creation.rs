//! SQLite receipts for the shared creation coordinator.

mod migration;

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use domain::creation::Receipt;
use domain::creation::protocol::Snapshot;
use model::ErrorCode;
use model::creation::{Creations, ReceiptStore};
use rusqlite::{Connection, OptionalExtension, Statement, params};

const UPSERT: &str = "INSERT INTO receipts (id, intent, snapshot, retry_initial_agent)
    VALUES (?1, ?2, ?3, ?4)
    ON CONFLICT(id) DO UPDATE SET intent = excluded.intent, snapshot = excluded.snapshot,
        retry_initial_agent = excluded.retry_initial_agent";

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("receipt file I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("receipt database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid receipt JSON at line {line}, column {column}")]
    Json { line: usize, column: usize },
    #[error("unsupported receipt database version {0}")]
    Version(i64),
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json {
            line: error.line(),
            column: error.column(),
        }
    }
}

#[derive(Debug)]
struct SqliteReceiptStore {
    connection: Mutex<Connection>,
}

impl SqliteReceiptStore {
    fn lock(&self) -> Result<MutexGuard<'_, Connection>, ErrorCode> {
        self.connection.lock().map_err(|_| ErrorCode::RegistryIo)
    }
}

impl ReceiptStore for SqliteReceiptStore {
    fn snapshots(&self) -> Result<Vec<Snapshot>, ErrorCode> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT snapshot FROM receipts")
            .map_err(|error| sql_error(&error))?;
        let rows = statement
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|error| sql_error(&error))?;
        rows.map(|row| {
            serde_json::from_slice(&row.map_err(|error| sql_error(&error))?)
                .map_err(|_| ErrorCode::RegistryIo)
        })
        .collect()
    }

    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode> {
        let connection = self.lock()?;
        let record = connection
            .prepare_cached(
                "SELECT intent, snapshot, retry_initial_agent FROM receipts WHERE id = ?1",
            )
            .and_then(|mut statement| {
                statement
                    .query_row([id], |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, Vec<u8>>(1)?,
                            row.get(2)?,
                        ))
                    })
                    .optional()
            })
            .map_err(|error| sql_error(&error))?;
        record
            .map(|(intent, snapshot, retry_initial_agent)| {
                Ok(Receipt {
                    id: id.to_owned(),
                    intent: serde_json::from_slice(&intent).map_err(|_| ErrorCode::RegistryIo)?,
                    snapshot: serde_json::from_slice(&snapshot)
                        .map_err(|_| ErrorCode::RegistryIo)?,
                    retry_initial_agent,
                })
            })
            .transpose()
    }

    fn put(&self, receipt: Receipt) -> Result<(), ErrorCode> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare_cached(UPSERT)
            .map_err(|error| sql_error(&error))?;
        write_receipt(&mut statement, &receipt).map_err(|error| match error {
            StoreError::Sql(error) => sql_error(&error),
            _ => ErrorCode::RegistryIo,
        })
    }
}

fn sql_error(error: &rusqlite::Error) -> ErrorCode {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull) {
        return ErrorCode::ResourceExhausted;
    }
    ErrorCode::RegistryIo
}

fn write_receipt(statement: &mut Statement<'_>, receipt: &Receipt) -> Result<(), StoreError> {
    statement.execute(params![
        receipt.id,
        serde_json::to_vec(&receipt.intent)?,
        serde_json::to_vec(&receipt.snapshot)?,
        receipt.retry_initial_agent,
    ])?;
    Ok(())
}

/// Open the SQLite database beside the former JSON file at `legacy_path`.
/// Import the JSON once, keep it as a backup, and restore claims without retrying interrupted work.
/// # Errors
/// Returns `RegistryIo` if the database or the initial import cannot be opened.
pub fn open(legacy_path: &Path) -> Result<Creations, ErrorCode> {
    let database = legacy_path.with_extension("sqlite3");
    let connection = super::sqlite::open::<StoreError>(&database)
        .and_then(|mut connection| {
            migration::initialize(&mut connection, legacy_path)?;
            Ok(connection)
        })
        .map_err(|error| {
            tracing::error!(database = %database.display(), legacy = %legacy_path.display(), %error,
                "could not open creation receipt store");
            ErrorCode::RegistryIo
        })?;
    Creations::with_store(Arc::new(SqliteReceiptStore {
        connection: Mutex::new(connection),
    }))
}

#[cfg(test)]
mod tests;
