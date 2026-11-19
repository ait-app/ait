//! One-time, streaming import. Only a complete, synced database replaces the old file.

use std::fmt;
use std::fs::{self, File};
use std::io::{BufReader, ErrorKind, Read, Seek};
use std::path::Path;

use domain::creation::Receipt;
use domain::creation::protocol::Snapshot;
use model::creation::fingerprint;
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use serde::de::{Deserializer, SeqAccess, Visitor};
use serde_json::Value;

use super::{StoreError, compressed, insert};

const HEADER: &[u8; 16] = b"SQLite format 3\0";
const APPLICATION_ID: u32 = u32::from_be_bytes(*b"AITC");

pub(super) fn open(path: &Path) -> Result<Connection, StoreError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let source = match File::open(path) {
        Ok(mut file) => {
            let mut header = [0; 16];
            match file.read_exact(&mut header) {
                Ok(()) if &header == HEADER => return existing(path),
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::UnexpectedEof => {}
                Err(error) => return Err(error.into()),
            }
            file.rewind()?;
            Some(file)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    fs::create_dir_all(parent)?;
    let filename = path.file_name().ok_or(StoreError::Format)?;
    let mut prefix = filename.to_os_string();
    prefix.push(".migrating");
    let staging_path = parent.join(&prefix);
    let mut journal = staging_path.as_os_str().to_os_string();
    journal.push("-journal");
    for leftover in [Path::new(&journal), &staging_path] {
        match fs::remove_file(leftover) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let staged = tempfile::Builder::new()
        .prefix(&prefix)
        .rand_bytes(0)
        .tempfile_in(parent)?;
    let mut db = Connection::open(staged.path())?;
    db.pragma_update(None, "auto_vacuum", "FULL")?;
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE receipts (
        id TEXT PRIMARY KEY, intent_hash BLOB NOT NULL, snapshot BLOB NOT NULL,
        retry INTEGER NOT NULL, revision BLOB NOT NULL, workspace_claim TEXT, agent_claim TEXT
    ) WITHOUT ROWID;
    CREATE INDEX workspace_owner ON receipts(workspace_claim) WHERE workspace_claim IS NOT NULL;
    CREATE INDEX agent_owner ON receipts(agent_claim) WHERE agent_claim IS NOT NULL;",
    )?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)?;
    tx.pragma_update(None, "user_version", 1)?;
    if let Some(file) = source {
        let mut json = serde_json::Deserializer::from_reader(BufReader::new(file));
        let mut import_error = None;
        let parsed = (&mut json).deserialize_seq(Import {
            db: &tx,
            error: &mut import_error,
        });
        if let Some(error) = import_error {
            return Err(error);
        }
        parsed?;
        json.end()?;
    }
    tx.commit()?;
    db.close().map_err(|(_, error)| error)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    existing(path)
}

fn existing(path: &Path) -> Result<Connection, StoreError> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let id: u32 = db.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if id != APPLICATION_ID || version != 1 {
        return Err(StoreError::Format);
    }
    db.prepare(
        "SELECT id, intent_hash, snapshot, retry, revision, workspace_claim, agent_claim
         FROM receipts LIMIT 0",
    )?;
    Ok(db)
}

#[derive(Deserialize)]
struct Legacy {
    id: String,
    intent: Value,
    snapshot: Snapshot,
    #[serde(default)]
    retry_initial_agent: bool,
}

struct Import<'a> {
    db: &'a Connection,
    error: &'a mut Option<StoreError>,
}

impl<'de> Visitor<'de> for Import<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a legacy creation receipt array")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while let Some(row) = seq.next_element::<Legacy>()? {
            let receipt = Receipt {
                id: row.id,
                intent_hash: fingerprint::of(&row.intent),
                snapshot: row.snapshot,
                retry_initial_agent: row.retry_initial_agent,
            };
            let result = compressed(&receipt.snapshot).and_then(|bytes| {
                insert(self.db, &receipt, &bytes, true).map_err(StoreError::from)
            });
            if let Err(error) = result {
                *self.error = Some(error);
                return Err(serde::de::Error::custom(
                    "could not import creation receipt",
                ));
            }
        }
        Ok(())
    }
}
