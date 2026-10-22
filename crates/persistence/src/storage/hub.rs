//! Atomic, bounded storage for the daemon-owned Host publication journal.

use std::path::PathBuf;

use model::hub::{OutboxStore, StoreError};
use serde_json::Value;

use crate::{Error, File};

const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// One private journal file under the daemon data directory; no device/provider credentials.
#[derive(Debug)]
pub struct FileOutbox(File);

impl FileOutbox {
    /// Select the journal path without performing I/O.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(File::new(path))
    }
}

impl OutboxStore for FileOutbox {
    fn load(&self) -> Result<Value, StoreError> {
        match self.0.read_limited(MAX_BYTES) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| StoreError),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Value::Null)
            }
            Err(Error::Io(_) | Error::TooLarge) => Err(StoreError),
        }
    }

    fn save(&self, document: &Value) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec(document).map_err(|_| StoreError)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(StoreError);
        }
        self.0.write(&bytes).map_err(|_| StoreError)
    }
}

#[cfg(test)]
mod tests;
