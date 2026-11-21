//! SQLite store of Workspace notepads (ADR-296).

use std::io;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use domain::workspace::notepad::{Note, NoteChange, Notepad};
use model::workspace::notepad::{NotepadStore, NotepadStoreError};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

const SCHEMA: &str = "CREATE TABLE notepads (
        workspace_id TEXT PRIMARY KEY,
        revision INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE notes (
        workspace_id TEXT NOT NULL,
        key TEXT NOT NULL,
        note BLOB NOT NULL,
        PRIMARY KEY (workspace_id, key)
    ) STRICT, WITHOUT ROWID;";

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("notepad file I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("notepad database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("invalid stored note: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported notepad database version {0}")]
    Version(i64),
    #[error("notepad revision out of range")]
    Revision,
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
    let revision = connection
        .prepare_cached("SELECT revision FROM notepads WHERE workspace_id = ?1")?
        .query_row([workspace_id], |row| row.get::<_, i64>(0))
        .optional()?
        .unwrap_or(0);
    let revision = u64::try_from(revision).map_err(|_| StoreError::Revision)?;
    let mut statement =
        connection.prepare_cached("SELECT note FROM notes WHERE workspace_id = ?1")?;
    let notes = statement
        .query_map([workspace_id], |row| row.get::<_, Vec<u8>>(0))?
        .map(|note| Ok(serde_json::from_slice::<Note>(&note?)?))
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(Notepad::restore(revision, notes))
}

fn commit(
    connection: &mut Connection,
    workspace_id: &str,
    revision: u64,
    change: &NoteChange,
) -> Result<(), StoreError> {
    let revision = i64::try_from(revision).map_err(|_| StoreError::Revision)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction
        .prepare_cached(
            "INSERT INTO notepads (workspace_id, revision) VALUES (?1, ?2)
            ON CONFLICT(workspace_id) DO UPDATE SET revision = excluded.revision",
        )?
        .execute(params![workspace_id, revision])?;
    match change {
        NoteChange::Put(note) => transaction
            .prepare_cached(
                "INSERT INTO notes (workspace_id, key, note) VALUES (?1, ?2, ?3)
                ON CONFLICT(workspace_id, key) DO UPDATE SET note = excluded.note",
            )?
            .execute(params![workspace_id, note.key, serde_json::to_vec(note)?])?,
        NoteChange::Remove(key) => transaction
            .prepare_cached("DELETE FROM notes WHERE workspace_id = ?1 AND key = ?2")?
            .execute(params![workspace_id, key])?,
    };
    Ok(transaction.commit()?)
}

fn clear(connection: &mut Connection, workspace_id: &str) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute("DELETE FROM notes WHERE workspace_id = ?1", [workspace_id])?;
    transaction.execute(
        "DELETE FROM notepads WHERE workspace_id = ?1",
        [workspace_id],
    )?;
    Ok(transaction.commit()?)
}

impl NotepadStore for SqliteNotepadStore {
    fn load(&self, workspace_id: &str) -> Result<Notepad, NotepadStoreError> {
        Ok(load(&self.lock(), workspace_id)?)
    }

    fn commit(
        &self,
        workspace_id: &str,
        revision: u64,
        change: &NoteChange,
    ) -> Result<(), NotepadStoreError> {
        Ok(commit(&mut self.lock(), workspace_id, revision, change)?)
    }

    fn clear(&self, workspace_id: &str) -> Result<(), NotepadStoreError> {
        Ok(clear(&mut self.lock(), workspace_id)?)
    }
}

#[cfg(test)]
mod tests;
