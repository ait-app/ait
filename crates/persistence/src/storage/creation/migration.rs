//! Transactional, one-time import of legacy JSON creation receipts.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;

use domain::creation::Receipt;
use rusqlite::{Connection, Statement, TransactionBehavior};
use serde::de::{self, DeserializeSeed, SeqAccess, Visitor};

use super::{StoreError, UPSERT, write_receipt};

pub(super) fn initialize(connection: &mut Connection, legacy: &Path) -> Result<(), StoreError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    match version {
        0 => {
            transaction.execute_batch(
                "CREATE TABLE receipts (
                    id TEXT PRIMARY KEY,
                    intent BLOB NOT NULL,
                    snapshot BLOB NOT NULL,
                    retry_initial_agent INTEGER NOT NULL
                ) STRICT;",
            )?;
            import_legacy(&mut transaction.prepare(UPSERT)?, legacy)?;
            transaction.pragma_update(None, "user_version", 1)?;
        }
        1 => {}
        unsupported => return Err(StoreError::Version(unsupported)),
    }
    transaction.commit()?;
    Ok(())
}

fn import_legacy(statement: &mut Statement<'_>, legacy: &Path) -> Result<(), StoreError> {
    let file = match File::open(legacy) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut decoder = serde_json::Deserializer::from_reader(BufReader::new(file));
    let mut import = Import {
        statement,
        error: None,
    };
    let result = (&mut import).deserialize(&mut decoder);
    if let Some(error) = import.error {
        return Err(error);
    }
    result?;
    decoder.end()?;
    Ok(())
}

struct Import<'a, 'connection> {
    statement: &'a mut Statement<'connection>,
    error: Option<StoreError>,
}

impl<'de> DeserializeSeed<'de> for &mut Import<'_, '_> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, decoder: D) -> Result<(), D::Error> {
        decoder.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for &mut Import<'_, '_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of creation receipts")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut entries: A) -> Result<(), A::Error> {
        while let Some(receipt) = entries.next_element::<Receipt>()? {
            if let Err(error) = write_receipt(self.statement, &receipt) {
                self.error = Some(error);
                return Err(de::Error::custom("could not import creation receipt"));
            }
        }
        Ok(())
    }
}
