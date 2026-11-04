//! Bounded, versioned atomic schedule storage.
use domain::schedule::Schedule;
use domain::storage::schedule::Error;
use model::storage::schedule::Store;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

/// Local JSON adapter; the independent host owns the containing data directory lease.
#[derive(Debug)]
pub struct FileStore {
    path: PathBuf,
}
#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    schedules: Vec<Schedule>,
}
impl FileStore {
    /// Configure an absolute private state path without writing it.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    fn check(&self) -> Result<(), Error> {
        let mut current = PathBuf::new();
        for part in self.path.components() {
            if matches!(part, std::path::Component::ParentDir) {
                return Err(Error::Storage);
            }
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(Error::Storage),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(Error::Storage),
            }
        }
        Ok(())
    }
}
impl Store for FileStore {
    fn load(&self) -> Result<Vec<Schedule>, Error> {
        self.check()?;
        let bytes = match crate::File::new(&self.path).read_limited(MAX_DOCUMENT_BYTES) {
            Ok(bytes) => bytes,
            Err(crate::Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(vec![]);
            }
            Err(crate::Error::Io(_) | crate::Error::TooLarge) => return Err(Error::Storage),
        };
        let doc: Document = serde_json::from_slice(&bytes).map_err(|_| Error::Storage)?;
        if !matches!(doc.version, 1 | 2) {
            return Err(Error::Storage);
        }
        Ok(doc.schedules)
    }
    fn save(&mut self, schedules: &[Schedule]) -> Result<(), Error> {
        self.check()?;
        let bytes = serde_json::to_vec(&Document {
            version: 2,
            schedules: schedules.to_vec(),
        })
        .map_err(|_| Error::Storage)?;
        if u64::try_from(bytes.len()).map_or(true, |size| size > MAX_DOCUMENT_BYTES) {
            return Err(Error::Conflict);
        }
        crate::File::new(&self.path)
            .write(&bytes)
            .map_err(|_| Error::Storage)
    }
}
#[cfg(test)]
mod tests;
