//! Indexed, transactional creation receipts. The historical filename stays stable across upgrade.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use domain::creation::Receipt;
use domain::creation::protocol::Snapshot;
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use model::ErrorCode;
use model::creation::{Creations, ReceiptStore};
use rusqlite::{Connection, OptionalExtension, params};

mod migration;

/// Safe startup diagnostics for receipt storage, including the affected path.
#[derive(Debug, thiserror::Error)]
#[error("open creation receipts at {path}: {cause}")]
pub struct OpenError {
    path: PathBuf,
    cause: StoreError,
}

#[derive(Debug, thiserror::Error)]
enum StoreError {
    #[error("receipt file I/O: {0}")]
    Io(#[from] io::Error),
    #[error("receipt database: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("invalid receipt JSON ({category:?}) at line {line}, column {column}")]
    Json {
        category: serde_json::error::Category,
        line: usize,
        column: usize,
    },
    #[error("unsupported creation receipt database")]
    Format,
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        if error.is_io() {
            return Self::Io(error.into());
        }
        // Serde's original message can quote private request content.
        Self::Json {
            category: error.classify(),
            line: error.line(),
            column: error.column(),
        }
    }
}

#[derive(Debug)]
struct Store(Mutex<Connection>);

fn storage_error(error: impl Into<StoreError>) -> ErrorCode {
    let error = error.into();
    tracing::error!(%error, "access creation receipt");
    if matches!(error, StoreError::Database(ref db) if db.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull))
    {
        ErrorCode::ResourceExhausted
    } else {
        ErrorCode::RegistryIo
    }
}

fn compressed(snapshot: &Snapshot) -> Result<Vec<u8>, StoreError> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    serde_json::to_writer(&mut encoder, snapshot)?;
    Ok(encoder.finish()?)
}

/// Import uses the original registry's last-record-wins rule. New admissions atomically
/// require a free key and free resource claims. An Agent borrows its Workspace.
fn insert(
    db: &Connection,
    receipt: &Receipt,
    snapshot: &[u8],
    importing: bool,
) -> rusqlite::Result<usize> {
    let progress = &receipt.snapshot;
    let (workspace, agent) = progress.resource_claims();
    db.execute(
        "INSERT OR REPLACE INTO receipts
         (id, intent_hash, snapshot, retry, revision, workspace_claim, agent_claim)
         SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7 WHERE ?8 OR (
             NOT EXISTS (SELECT 1 FROM receipts WHERE id=?1) AND
             NOT EXISTS (SELECT 1 FROM receipts WHERE workspace_claim=?6) AND
             NOT EXISTS (SELECT 1 FROM receipts WHERE agent_claim=?7))",
        params![
            receipt.id,
            receipt.intent_hash,
            snapshot,
            receipt.retry_initial_agent,
            progress.revision.to_be_bytes(),
            workspace,
            agent,
            importing
        ],
    )
}

impl ReceiptStore for Store {
    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode> {
        let db = self.0.lock().map_err(|_| ErrorCode::RegistryIo)?;
        let row = db
            .query_row(
                "SELECT intent_hash,snapshot,retry FROM receipts WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get::<_, Vec<u8>>(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        row.map(|(intent_hash, bytes, retry_initial_agent)| {
            let snapshot =
                serde_json::from_reader(io::BufReader::new(ZlibDecoder::new(bytes.as_slice())))
                    .map_err(storage_error)?;
            Ok(Receipt {
                id: id.to_owned(),
                intent_hash,
                snapshot,
                retry_initial_agent,
            })
        })
        .transpose()
    }

    fn put(&self, receipt: Receipt, expected_revision: Option<u64>) -> Result<(), ErrorCode> {
        let bytes = compressed(&receipt.snapshot).map_err(storage_error)?;
        let db = self.0.lock().map_err(|_| ErrorCode::RegistryIo)?;
        // Each single SQL statement commits its row and both claim indexes atomically.
        let changed = if let Some(expected) = expected_revision {
            db.execute(
                "UPDATE receipts SET snapshot=?1, retry=?2, revision=?3
                 WHERE id=?4 AND revision=?5",
                params![
                    bytes,
                    receipt.retry_initial_agent,
                    receipt.snapshot.revision.to_be_bytes(),
                    receipt.id,
                    expected.to_be_bytes()
                ],
            )
        } else {
            insert(&db, &receipt, &bytes, false)
        }
        .map_err(storage_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ErrorCode::IdempotencyConflict)
        }
    }
}

/// Open the indexed receipts at `path` under the host's exclusive data-directory lease.
/// A legacy JSON array is migrated atomically before the coordinator admits work.
/// # Errors
/// Returns the path and a content-safe cause if loading, validation or installation fails.
pub fn open(path: PathBuf) -> Result<Creations, OpenError> {
    let db = migration::open(&path).map_err(|cause| OpenError { path, cause })?;
    Ok(Creations::with_store(Arc::new(Store(Mutex::new(db)))))
}

#[cfg(test)]
mod tests;
