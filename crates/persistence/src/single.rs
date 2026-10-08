use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::watch::Watch;

/// Failure to access or observe a single file.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Reading, staging, or installing the file failed.
    #[error("file I/O failed")]
    Io(#[from] std::io::Error),
    /// A bounded read exceeded the caller's byte limit.
    #[error("file exceeds the read limit")]
    TooLarge,
    /// A polling interval must be positive.
    #[error("file observation interval must be positive")]
    InvalidInterval,
    /// File observation requires a Tokio runtime.
    #[error("file observation requires a Tokio runtime")]
    RuntimeUnavailable,
    /// The observation task stopped or its runtime shut down.
    #[error("file observation closed")]
    WatchClosed,
}

/// An owned path supporting blocking reads, atomic writes, and asynchronous observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    path: PathBuf,
}

impl File {
    /// Select `path` without performing I/O.
    /// Returns a handle; relative paths retain their caller-provided spelling.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Return the selected path without resolving symlinks or creating it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the complete file into owned bytes.
    /// # Errors
    /// Returns filesystem errors, including `NotFound` for an absent file.
    pub fn read(&self) -> Result<Vec<u8>, Error> {
        fs::read(&self.path).map_err(Into::into)
    }

    /// Read at most `limit` bytes, rejecting larger files without loading their entire contents.
    /// # Errors
    /// Returns filesystem errors or `TooLarge` when the file exceeds `limit`.
    pub fn read_limited(&self, limit: u64) -> Result<Vec<u8>, Error> {
        let file = fs::File::open(&self.path)?;
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).map_or(true, |length| length > limit) {
            return Err(Error::TooLarge);
        }
        Ok(bytes)
    }

    /// Read the complete file as UTF-8 text.
    /// # Errors
    /// Returns filesystem errors or invalid UTF-8 errors without echoing file contents.
    pub fn read_text(&self) -> Result<String, Error> {
        fs::read_to_string(&self.path).map_err(Into::into)
    }

    /// Atomically replace the file with `bytes`, creating missing parent directories.
    /// The temporary file resides beside the destination and is synced before replacement.
    /// # Errors
    /// Returns staging, sync, or replacement errors. Hosts own cross-process write exclusion.
    pub fn write(&self, bytes: &[u8]) -> Result<(), Error> {
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    }

    /// Observe creation, modification, replacement, and removal at `interval` cadence.
    /// Polling reads metadata outside the reactor; it coalesces changes to the latest observation.
    /// Dropping the returned handle cancels its task. Intermediate edits between polls may coalesce.
    /// # Errors
    /// Returns `InvalidInterval`, `RuntimeUnavailable`, or an initial task failure.
    pub async fn watch(&self, interval: Duration) -> Result<Watch, Error> {
        Watch::start(self.clone(), interval).await
    }
}

#[cfg(test)]
mod tests;
