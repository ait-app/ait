//! SQLite store of Workspace notepads (ADR-296).

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use domain::workspace::notepad::Notepad;
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

const SCHEMA: &str = "CREATE TABLE notepads (
        workspace_id TEXT PRIMARY KEY,
        notepad BLOB NOT NULL
    ) STRICT;";

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("notepad file I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("notepad database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid stored notepad: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported notepad database version {0}")]
    Version(i64),
}

impl From<StoreError> for NotepadStoreError {
    fn from(error: StoreError) -> Self {
        let full = matches!(&error, StoreError::Sql(error)
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull));
        tracing::error!(%error, "notepad store operation failed");
        if full { Self::Full } else { Self::Io }
    }
}

/// Every Workspace's notepad in one SQLite database.
#[derive(Debug)]
pub struct SqliteNotepadStore {
    connection: Mutex<Connection>,
}

impl SqliteNotepadStore {
    /// Open the notepad database at `database`, creating it when absent.
    /// # Errors
    /// Returns [`NotepadStoreError`] when the file cannot be created or opened, or was written by
    /// a newer version.
    pub fn open(database: &Path) -> Result<Self, NotepadStoreError> {
        let connection = super::sqlite::open::<StoreError>(database).and_then(initialize)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn initialize(mut connection: Connection) -> Result<Connection, StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => {
            transaction.execute_batch(SCHEMA)?;
            transaction.pragma_update(None, "user_version", 1)?;
        }
        1 => {}
        unsupported => return Err(StoreError::Version(unsupported)),
    }
    transaction.commit()?;
    Ok(connection)
}

fn load(connection: &Connection, workspace_id: &str) -> Result<Notepad, StoreError> {
    let stored = connection
        .prepare_cached("SELECT notepad FROM notepads WHERE workspace_id = ?1")?
        .query_row([workspace_id], |row| row.get::<_, Vec<u8>>(0))
        .optional()?;
    Ok(match stored {
        Some(notepad) => serde_json::from_slice(&notepad)?,
        None => Notepad::default(),
    })
}

fn save(connection: &Connection, workspace_id: &str, notepad: &Notepad) -> Result<(), StoreError> {
    connection
        .prepare_cached(
            "INSERT INTO notepads (workspace_id, notepad) VALUES (?1, ?2)
            ON CONFLICT(workspace_id) DO UPDATE SET notepad = excluded.notepad",
        )?
        .execute(params![workspace_id, serde_json::to_vec(notepad)?])?;
    Ok(())
}

impl NotepadStore for SqliteNotepadStore {
    fn load(&self, workspace_id: &str) -> Result<Notepad, NotepadStoreError> {
        Ok(load(&self.lock(), workspace_id)?)
    }

    fn save(&self, workspace_id: &str, notepad: &Notepad) -> Result<(), NotepadStoreError> {
        Ok(save(&self.lock(), workspace_id, notepad)?)
    }

    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError> {
        self.lock()
            .execute(
                "DELETE FROM notepads WHERE workspace_id = ?1",
                [workspace_id],
            )
            .map(drop)
            .map_err(|error| StoreError::Sql(error).into())
    }
}

#[cfg(test)]
mod tests;
