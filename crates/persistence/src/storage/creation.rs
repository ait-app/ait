//! Atomic JSON receipts for the shared creation coordinator.

use std::path::PathBuf;
use std::sync::Arc;

use domain::creation::Receipt;
use model::ErrorCode;
use model::creation::{Creations, ReceiptStore};

use crate::registry::FileRegistry;

/// File-backed creation receipts; clones share the committed cache and serialization lock.
#[derive(Debug, Clone)]
struct FileReceiptStore {
    registry: Arc<FileRegistry<Receipt>>,
}

impl FileReceiptStore {
    /// Select the JSON array at `path` without creating or reading it.
    #[must_use]
    fn new(path: PathBuf) -> Self {
        Self {
            registry: Arc::new(FileRegistry::new(path, |receipt| &receipt.id)),
        }
    }
}

impl ReceiptStore for FileReceiptStore {
    fn list(&self) -> Result<Vec<Receipt>, ErrorCode> {
        self.registry.list().map_err(|_| ErrorCode::RegistryIo)
    }

    fn get(&self, id: &str) -> Result<Option<Receipt>, ErrorCode> {
        self.registry.get(id).map_err(|_| ErrorCode::RegistryIo)
    }

    fn put(&self, receipt: Receipt) -> Result<(), ErrorCode> {
        self.registry
            .mutate(|records| {
                records.insert(receipt.id.clone(), receipt);
                Ok(((), true))
            })
            .map_err(|_| ErrorCode::RegistryIo)
    }
}

/// Open receipts at `path` and restore creation claims without retrying interrupted work.
/// Returns the shared application coordinator with this file adapter injected.
/// # Errors
/// Returns `RegistryIo` if the receipt file is malformed or inaccessible.
pub fn open(path: PathBuf) -> Result<Creations, ErrorCode> {
    Creations::with_store(Arc::new(FileReceiptStore::new(path)))
}

#[cfg(test)]
mod tests;
